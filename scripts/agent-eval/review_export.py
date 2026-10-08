"""Project every frozen assignment, with adverse outcomes and conditional comparisons."""

from collections import Counter, defaultdict
from pathlib import Path

from accounting import fingerprint, read_json, require, sha256
from review_campaign import SCRIPT_ROOT, campaign_lock, file_hash, inventory, load_plan, write_new

USAGE_FIELDS = ("input_tokens", "cached_input_tokens", "cache_write_input_tokens", "output_tokens", "reasoning_output_tokens", "total_tokens")
REQUESTED_TOKEN_FIELDS = ("input_tokens", "cache_write_input_tokens", "cached_input_tokens", "output_tokens")
RESULT_FIELDS = ("status", "exit_code", "returncode",
                 "peak_rss_bytes", "profile_sha256", "stdout_sha256", "trace_sha256", "stdout_bytes",
                 "stream_complete", "stdout_truncated", "process_tree_drained", "termination_reason",
                 "usage_scope", "cli_version", "input_receipt", "campaign_fatal", "failure_phase")
ACCOUNTING_FIELDS = ("schema", "adapter", "kind", "invocation_count", "usage_basis", "observed_usage",
                     "known_usage_prefix",
                     "comparable_usage", "comparable_fields",
                     "usage_complete", "provider_call_ids_available", "unknown_fields", "episode_errors",
                     "command_statistics", "money", "evidence_sha256")
INPUT_IDENTITIES = {"workspace", "codex", "codex_runtime", "controller_ca", "reposcout", "skill", "runtime_tools"}


def _optional_hash(value):
    return None if value is None else sha256(value, "input identity")


def project_input_receipt(receipt):
    if receipt is None:
        return None
    require(isinstance(receipt, dict) and set(receipt) <= INPUT_IDENTITIES | {"codex_runtime_assets", "runtime_tool_assets"},
            "unsupported input receipt fields")
    result = {}
    for name, value in receipt.items():
        if name in ("codex_runtime_assets", "runtime_tool_assets"):
            require(isinstance(value, dict), "runtime asset identities must be an object")
            assets = {}
            for relative, digest in value.items():
                require(isinstance(relative, str) and relative and not relative.startswith("/")
                        and "\\" not in relative and all(part not in ("", ".", "..") for part in relative.split("/")),
                        "runtime asset identity must use a relative path")
                assets[relative] = sha256(digest, "runtime asset identity")
            result[name] = assets
        else:
            require(isinstance(value, dict) and set(value) == {"sha256", "expected_sha256", "matched"},
                    "input receipt must contain only hash identity fields")
            require(value["matched"] is None or type(value["matched"]) is bool, "invalid input match status")
            result["workspace_identity" if name == "workspace" else name] = {
                "sha256": sha256(value["sha256"], "copied input identity"),
                "expected_sha256": _optional_hash(value["expected_sha256"]), "matched": value["matched"]}
    return result


def project_configuration(configuration):
    if configuration is None:
        return None
    result = dict(configuration)
    if "expected_inputs" in result:
        expected = result["expected_inputs"]
        require(isinstance(expected, dict) and set(expected) <= INPUT_IDENTITIES, "unsupported expected input identity")
        result["expected_inputs"] = {"workspace_identity" if name == "workspace" else name: _optional_hash(value)
                                     for name, value in expected.items()}
    return result


def safe_projection(value):
    """Fail closed if an explicit projection accidentally carries private evidence."""
    if isinstance(value, dict):
        for key, item in value.items():
            require(key not in {"prompt", "answers", "answer", "stdout", "stderr", "command", "commands",
                                "workspace", "controller_dir", "auth_file", "auth", "run_dir", "private_oracle_path",
                                "stdout_path", "stderr_path", "answer_path", "adjudication_packet", "source",
                                "quote", "source_snapshots"},
                    "private evidence field in sanitized export")
            safe_projection(item)
    elif isinstance(value, list):
        for item in value:
            safe_projection(item)
    elif isinstance(value, str):
        value = value.replace("/home/eval", "[sandbox-home]")
        require(not any(marker in value for marker in ("/home/", "/Users/", "/tmp/", "/private/", "/var/tmp/")),
                "absolute host path in sanitized export")


def _adjudication_index(path):
    if path is None:
        return {}
    value = read_json(path)
    rows = value.get("adjudications") if isinstance(value, dict) else value
    require(isinstance(rows, list) and len(rows) <= 60, "invalid bounded adjudication inventory")
    result = {}
    for row in rows:
        require(isinstance(row, dict) and isinstance(row.get("packet_id"), str), "invalid adjudication")
        require(row["packet_id"] not in result, "duplicate blinded adjudication")
        result[row["packet_id"]] = row
    return result


def _apply_adjudications(runs, index):
    from review_grading import grade_episode
    matched = set()
    for run in runs:
        grade = run.get("quality")
        if not grade:
            continue
        packet = grade.get("packet_id")
        if packet not in index:
            continue
        adjudication = index[packet]
        require(adjudication.get("answer_sha256") == grade.get("answer_sha256"), "adjudication answer hash mismatch")
        directory = Path(run["assignment"]["run_dir"])
        steps = sorted((directory).glob("step-*/case.json"))
        require(steps, "missing private case provenance for adjudication")
        record = read_json(steps[-1])
        answers = [read_json(Path(invocation["result"]["answer_path"])) for invocation in run["invocations"]]
        executions = (grade.get("adjudication_packet") or {}).get("execution_evidence", [])
        revised = grade_episode(record, answers, adjudication=adjudication, execution_evidence=executions)
        require(revised["packet_id"] == packet and revised["answer_sha256"] == grade["answer_sha256"],
                "private answer or packet changed")
        run["quality"] = revised
        run["adjudication_sha256"] = fingerprint(adjudication)
        run["adjudication_blinded"] = adjudication.get("blinded")
        run["adjudicator_identity_sha256"] = fingerprint(adjudication.get("reviewer"))
        matched.add(packet)
    require(matched == set(index), "unknown blinded packet in adjudication import")


def _quality_projection(grade):
    if grade is None:
        return None
    projected = {key: grade.get(key) for key in ("schema", "packet_id", "answer_sha256", "adjudication_status", "quality",
                                               "validation_claims_checked", "validation_execution_verified")}
    packet = grade.get("adjudication_packet") or {}
    projected["execution_evidence_sha256"] = packet.get("execution_evidence_sha256")
    projected["execution_evidence_count"] = len(packet.get("execution_evidence", []))
    automatic = grade.get("automatic") or {}
    projected["automatic"] = {key: value for key, value in automatic.items() if key != "steps"}
    projected["automatic"]["steps"] = []
    for step in automatic.get("steps", []):
        row = {key: value for key, value in step.items() if key != "evidence"}
        row["evidence"] = [{key: item.get(key) for key in ("path", "side", "snapshot", "start_line", "end_line",
                                                          "source_sha256", "origin_step", "retained")}
                           for item in step.get("evidence", [])]
        projected["automatic"]["steps"].append(row)
    return projected


def project_run(run):
    assignment = run["assignment"]
    projected = {key: assignment[key] for key in ("run_id", "pair_id", "repeat_id", "case_id", "variant",
                                                 "sequence", "pair_position", "fixture_sha256", "step_count")}
    projected.update({"status": run["status"], "oracle_sha256": assignment.get("oracle_sha256"),
                      "initial_prompt": assignment.get("initial_prompt"), "quality": _quality_projection(run.get("quality")),
                      "configuration": project_configuration(assignment.get("configuration")),
                      "configuration_sha256": assignment.get("configuration_sha256"),
                      "planned_step_prompts": assignment.get("step_prompts"),
                      "adjudication_sha256": run.get("adjudication_sha256"),
                      "adjudication_blinded": run.get("adjudication_blinded"),
                      "adjudicator_identity_sha256": run.get("adjudicator_identity_sha256"),
                      "answer_hashes": run.get("answers", []), "errors": run.get("errors", []),
                      "campaign_fatal": run.get("campaign_fatal"),
                      "recovery_errors": run.get("recovery_errors", []),
                      "invocations": []})
    for invocation in run.get("invocations", []):
        result = invocation["result"]
        result_projection = {key: result.get(key) for key in RESULT_FIELDS if key != "input_receipt"}
        result_projection["input_receipt"] = project_input_receipt(result.get("input_receipt"))
        projected["invocations"].append({"step_id": invocation["step_id"],
                                          "prompt_hashes": invocation.get("prompt_hashes"),
                                          "thread_id_sha256": fingerprint(result["thread_id"]) if result.get("thread_id") else None,
                                          "result": result_projection})
    accounting = run.get("accounting")
    projected["accounting"] = {key: accounting.get(key) for key in ACCOUNTING_FIELDS} if accounting else None
    if accounting:
        projected["accounting"]["invocations"] = [
            {key: trace.get(key) for key in ("trace_sha256", "trace_bytes", "status", "usage_scope", "usage_basis",
                                            "observed_usage", "comparable_usage",
                                            "comparable_fields", "usage_complete", "unknown_fields", "validation_errors",
                                            "emitted_usage_known_sum", "emitted_usage_known_sum_basis", "evidence_sha256")}
            for trace in accounting.get("invocations", [])]
    projected["missing_accounting_invocations"] = max(0, len(run.get("invocations", [])) -
                                                       (accounting.get("invocation_count", 0) if accounting else 0))
    safe_projection(projected)
    return projected


def quality_state(run):
    grade = run.get("quality")
    if grade is None:
        return "unavailable"
    if grade.get("adjudication_status") != "accepted":
        return "pending-adjudication"
    quality = grade.get("quality") or {}
    return "passed" if quality.get("passed") is True else "failed"


def _comparison(reference, candidate):
    reasons = []
    for run in (reference, candidate):
        if run["status"] != "completed":
            reasons.append(run["variant"] + ":" + run["status"])
        if quality_state(run) != "passed":
            reasons.append(run["variant"] + ":quality-" + quality_state(run))
        quality = (run.get("quality") or {}).get("quality") or {}
        if quality.get("regressions") or quality.get("missing_evidence"):
            reasons.append(run["variant"] + ":quality-evidence-gaps")
        accounting = run.get("accounting") or {}
        if accounting.get("comparable_usage") is not True or run.get("missing_accounting_invocations", 0):
            reasons.append(run["variant"] + ":usage-not-comparable")
    left, right = reference.get("accounting") or {}, candidate.get("accounting") or {}
    if left.get("usage_basis") != right.get("usage_basis"):
        reasons.append("usage-basis-differs")
    left_versions = {item["result"].get("cli_version") for item in reference["invocations"]}
    right_versions = {item["result"].get("cli_version") for item in candidate["invocations"]}
    if len(left_versions) != 1 or left_versions != right_versions or None in left_versions:
        reasons.append("cli-version-differs-or-missing")
    left_threads = {item.get("thread_id_sha256") for item in reference["invocations"]}
    right_threads = {item.get("thread_id_sha256") for item in candidate["invocations"]}
    if None in left_threads or None in right_threads or left_threads & right_threads:
        reasons.append("shared-or-missing-thread-identity")
    for key in ("fixture_sha256", "oracle_sha256", "step_count"):
        if reference.get(key) != candidate.get(key):
            reasons.append(key + "-differs")
    left_tasks = [item.get("prompt_hashes", {}).get("task_sha256") for item in reference["invocations"]]
    right_tasks = [item.get("prompt_hashes", {}).get("task_sha256") for item in candidate["invocations"]]
    if left_tasks != right_tasks or len(left_tasks) != reference["step_count"]:
        reasons.append("task-or-step-inventory-differs")
    fields = set(left.get("comparable_fields") or []) & set(right.get("comparable_fields") or [])
    if not {"input_tokens", "output_tokens"} <= fields:
        reasons.append("core-token-fields-not-comparable")
    eligible = not reasons
    deltas = {}
    if eligible:
        for field in sorted(fields):
            left_value = left.get("observed_usage", {}).get(field)
            right_value = right.get("observed_usage", {}).get(field)
            require(type(left_value) is int and type(right_value) is int, "comparable token field is unavailable")
            deltas[field] = right_value - left_value
    field_comparisons = {}
    for field in REQUESTED_TOKEN_FIELDS:
        known = field in fields
        field_comparisons[field] = {
            "eligible": eligible and known,
            "reference_value": left.get("observed_usage", {}).get(field),
            "candidate_value": right.get("observed_usage", {}).get(field),
            "delta": deltas.get(field),
            "ineligibility_reasons": ([] if eligible else ["pair-ineligible"])
                                     + ([] if known else ["field-not-comparable-in-both-arms"]),
        }
    return {"reference_run": reference["run_id"], "candidate_run": candidate["run_id"],
            "reference_variant": reference["variant"], "candidate_variant": candidate["variant"],
            "eligible": eligible, "ineligibility_reasons": sorted(set(reasons)),
            "usage_basis": left.get("usage_basis") if eligible else None, "token_deltas": deltas or None,
            "field_comparisons": field_comparisons,
            "all_requested_token_fields_comparable": eligible and set(REQUESTED_TOKEN_FIELDS) <= fields}


def comparisons(runs):
    pairs = defaultdict(dict)
    for run in runs:
        require(run["variant"] not in pairs[run["pair_id"]], "duplicate paired arm")
        pairs[run["pair_id"]][run["variant"]] = run
    result = []
    for pair_id, arms in sorted(pairs.items()):
        for reference, candidate in (("baseline", "reposcout"), ("baseline", "reposcout-cli"), ("reposcout", "reposcout-cli")):
            if candidate == "reposcout-cli" and candidate not in arms:
                continue
            if reference not in arms or candidate not in arms:
                result.append({"pair_id": pair_id, "eligible": False, "ineligibility_reasons": ["missing-assigned-arm"]})
            else:
                result.append({"pair_id": pair_id, **_comparison(arms[reference], arms[candidate])})
    return result


def all_run_costs(runs):
    """Retain costs on failures; partition totals by their measured accounting basis."""
    groups = defaultdict(list)
    for run in runs:
        accounting = run.get("accounting") or {}
        groups[(run["variant"], accounting.get("usage_basis") or "unavailable")].append(run)
    result = []
    for (variant, basis), selected in sorted(groups.items()):
        token_totals = {}
        known_counts = {}
        partial_totals = {}
        partial_counts = {}
        for field in USAGE_FIELDS:
            values = [(run.get("accounting") or {}).get("observed_usage", {}).get(field) for run in selected]
            measured = [value for value in values if type(value) is int]
            token_totals[field] = sum(measured) if measured else None
            known_counts[field] = len(measured)
            partial_values = [((run.get("accounting") or {}).get("known_usage_prefix") or {})
                              .get("observed_usage", {}).get(field)
                              for run, value in zip(selected, values) if value is None]
            partial = [value for value in partial_values if type(value) is int]
            partial_totals[field] = sum(partial) if partial else None
            partial_counts[field] = len(partial)
        result.append({"variant": variant, "usage_basis": basis, "assigned_runs": len(selected),
                       "status_counts": dict(Counter(run["status"] for run in selected)),
                       "observed_token_known_sums": token_totals, "token_known_run_counts": known_counts,
                       "observed_partial_token_known_sums": partial_totals,
                       "partial_token_known_run_counts": partial_counts,
                       "provider_charge": None,
                       "subscription_charge": None, "money_basis": "not-reported-by-exec-stream",
                       "full_provider_ledger_complete_runs": sum((run.get("accounting") or {}).get("usage_complete") is True for run in selected),
                       "missing_accounting_runs": sum(run.get("accounting") is None for run in selected),
                       "missing_accounting_invocations": sum(run["missing_accounting_invocations"] for run in selected)})
    return result


def report(root, adjudications=None):
    plan = load_plan(root)
    with campaign_lock(root):
        private_runs = inventory(plan)
    index = _adjudication_index(adjudications)
    if index:
        expected_grader = plan["pins"].get("harness_sources", {}).get("review_grading.py")
        if expected_grader:
            require(file_hash(SCRIPT_ROOT / "review_grading.py") == expected_grader,
                    "adjudication grader differs from frozen campaign")
    _apply_adjudications(private_runs, index)
    runs = [project_run(run) for run in private_runs]
    paired = comparisons(runs)
    pins = {key: value for key, value in plan["pins"].items()
            if key not in ("codex_binary", "reposcout_binary", "skill_dir", "controller_ca_file", "runtime_tool_paths")}
    result = {"schema": 1, "kind": "codex-review-campaign-report", "plan_sha256": plan["plan_sha256"],
              "stage": plan["stage"], "seed": plan["seed"], "repetitions": plan["repetitions"],
              "ablation": plan["ablation"], "model": plan["model"], "effort": plan["effort"], "pins": pins,
              "fixture_signer": plan.get("fixture_signer"),
              "assignment_count": len(runs), "status_counts": dict(Counter(run["status"] for run in runs)),
              "quality_counts": dict(Counter(quality_state(run) for run in runs)), "runs": runs,
              "all_assigned_run_costs": all_run_costs(runs), "pair_inventory": paired,
              "conditional_quality_matched_pairs": [pair for pair in paired if pair["eligible"]],
              "incomplete_main_pairs": sum(any(arms["status"] != "completed"
                                               for arms in runs if arms["pair_id"] == pair["pair_id"] and arms["variant"] in ("baseline", "reposcout"))
                                            for pair in paired if pair.get("candidate_variant") == "reposcout"),
              "statistical_inference": False, "synthetic_fixtures": True,
              "measurement_limits": ["complete provider-call ledger and subscription charge unavailable",
                                     "quality requires independent semantic adjudication with treatment/cost labels withheld",
                                     "answer prose or observed command text may reveal tool identity despite packet blinding",
                                     "input includes cache reads; cache-read and cache-write counts are separate, never added to input",
                                     "token comparisons are field-specific; unknown cache counters remain incomparable",
                                     "observed token sums include all measured statuses and are partitioned by basis",
                                     "conditional pairs exclude adverse or unmatched quality and are reported separately"]}
    safe_projection(result)
    return result


def export(root, destination, adjudications=None):
    result = report(root, adjudications)
    destination = Path(destination)
    require(not destination.exists() and not destination.is_symlink(), "export destination already exists")
    require(all(not parent.is_symlink() for parent in destination.absolute().parents), "export path traverses a symlink")
    destination.mkdir(parents=True)
    write_new(destination / "results.json", result)
    write_new(destination / "integrity.json", {"schema": 1, "plan_sha256": result["plan_sha256"],
                                               "results_canonical_json_sha256": fingerprint(result)})
    return {"plan_sha256": result["plan_sha256"], "assignment_count": result["assignment_count"],
            "status_counts": result["status_counts"], "results_canonical_json_sha256": fingerprint(result)}


def packets(root, destination):
    """Write private, shuffled reviewer packets without run IDs, arm or measured cost."""
    import random
    plan = load_plan(root)
    by_id = {}
    with campaign_lock(root):
        runs = inventory(plan)
    for run in runs:
        if run.get("quality"):
            packet = run["quality"]["adjudication_packet"]
            previous = by_id.get(packet["packet_id"])
            require(previous is None or previous == packet, "conflicting blinded packet identity")
            by_id[packet["packet_id"]] = packet
    selected = list(by_id.values())
    random.Random(plan["seed"] ^ 0xB11D).shuffle(selected)
    destination = Path(destination)
    require(not destination.exists() and not destination.is_symlink(), "packet destination already exists")
    require(all(not parent.is_symlink() for parent in destination.absolute().parents), "packet path traverses a symlink")
    destination.mkdir(mode=0o700)
    for packet in selected:
        sha256(packet["packet_id"], "blinded packet ID")
        require(not any(key in packet for key in ("variant", "run_id", "usage", "cost", "run_dir")), "packet reveals treatment identity")
        write_new(destination / (packet["packet_id"] + ".json"), packet)
    return {"packet_count": len(selected), "blinded": True}
