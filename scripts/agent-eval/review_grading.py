"""Verify pinned evidence and domain witnesses, then require blinded semantic adjudication.

Exact source checks establish provenance, not the truth of arbitrary causal prose. Known witness
contradictions are failures; other valid witness forms and evidence chains need explicit review.
"""

import hashlib
import json

from accounting import InvalidLedger, fingerprint, object_pairs, read_json, require, sha256
from review_cases import _safe_path, answer_schema

MAX_ANSWER_BYTES = 1024 * 1024


def _validate_schema(value, schema, label="answer"):
    kind = schema["type"]
    valid = {"object": isinstance(value, dict), "array": isinstance(value, list),
             "string": isinstance(value, str), "integer": type(value) is int}[kind]
    require(valid, label + ": expected " + kind)
    if "enum" in schema:
        require(value in schema["enum"], label + ": unknown value")
    if kind == "object":
        require(set(value) == set(schema["required"]), label + ": missing or unexpected fields")
        for key, item in value.items():
            _validate_schema(item, schema["properties"][key], label + "." + key)
    elif kind == "array":
        require(len(value) <= 128, label + ": too many entries")
        for index, item in enumerate(value):
            _validate_schema(item, schema["items"], f"{label}[{index}]")


def _source(oracle, record, step_index, side, path):
    require(side in ("base", "head"), "unknown snapshot side")
    _safe_path(path)
    revision = record["steps"][step_index][side + "_revision"]
    files = oracle["snapshots"][revision]
    require(path in files, "evidence path absent from requested snapshot")
    return files[path]


def _location(item, oracle, record, step_index):
    source = _source(oracle, record, step_index, item["side"], item["path"])
    require(item["snapshot"] == record["steps"][step_index][item["side"] + "_tree"],
            "evidence snapshot differs from requested tree")
    lines = source.splitlines(keepends=True)
    start, end = item["start_line"], item["end_line"]
    require(type(start) is int and type(end) is int and 1 <= start <= end <= len(lines),
            "evidence line range outside captured source")
    return source, "".join(lines[start - 1:end])


def _evidence(item, oracle, record, step_index):
    source, span = _location(item, oracle, record, step_index)
    require(bool(item["quote"].strip()), "empty source quote")
    require(item["quote"] in (span, span.removesuffix("\n")), "quoted evidence differs from captured source")
    return {**item, "source_sha256": hashlib.sha256(source.encode()).hexdigest(),
            "origin_step": step_index, "retained": False}


def _covers(evidence, target, oracle, record, step_index):
    if evidence["path"] != target["path"] or evidence["start_line"] > target["start_line"] or evidence["end_line"] < target["end_line"]:
        return False
    source = _source(oracle, record, step_index, target["side"], target["path"])
    return evidence["source_sha256"] == hashlib.sha256(source.encode()).hexdigest()


def _overlaps(location, target):
    return (location["side"] == target["side"] and location["path"] == target["path"]
            and location["start_line"] <= target["end_line"] and location["end_line"] >= target["start_line"])


def _json_value(value):
    def invalid(constant):
        raise ValueError("non-finite JSON value: " + constant)
    return json.loads(value, object_pairs_hook=object_pairs, parse_constant=invalid)


def _result_value(value):
    if isinstance(value, dict) and len(value) == 1 and next(iter(value)) in (
            "allowed", "renewed", "shipping_fee_cents", "amount_cents", "status"):
        return next(iter(value.values()))
    return value


def _witness(trigger, defects):
    observed = {key: _json_value(trigger[key + "_json"]) for key in ("input", "expected", "actual")}
    same_inputs = [(defect["id"], witness) for defect in defects for witness in defect["witnesses"]
                   if fingerprint(observed["input"]) == fingerprint(witness["input"])]
    matches = {identity for identity, witness in same_inputs
               if all(fingerprint(_result_value(observed[key])) == fingerprint(_result_value(witness[key]))
                      for key in ("expected", "actual"))}
    return observed, matches, bool(same_inputs) and not matches


def _issue(step, code, detail, **extra):
    return {"step_id": step, "code": code, "detail": detail, **extra}


def _execution_evidence(records, step_count):
    result = []
    for record in records or []:
        require(isinstance(record, dict), "invalid execution evidence")
        required = {"step_id", "command", "exit_code", "trace_sha256", "output_sha256", "output_excerpt"}
        require(set(record) == required, "execution evidence must use the sanitized controller schema")
        require(type(record["step_id"]) is int and 0 <= record["step_id"] < step_count,
                "execution evidence has unknown step")
        command = record["command"]
        require((isinstance(command, str) and command.strip()) or
                (isinstance(command, list) and command and all(isinstance(part, str) for part in command)),
                "execution evidence needs the actual command")
        require(type(record["exit_code"]) is int, "execution evidence needs an observed exit status")
        for field in ("trace_sha256", "output_sha256"):
            sha256(record[field], field)
        require(isinstance(record["output_excerpt"], str) and len(record["output_excerpt"].encode()) <= 4096,
                "execution output excerpt exceeds bound")
        result.append(dict(record))
    require(len(result) <= 128, "too many fixture execution records")
    return result


def grade_episode(record, answers, adjudication=None, *, execution_evidence=None):
    oracle = read_json(record["private_oracle_path"])
    require(fingerprint(oracle) == record["oracle_sha256"], "private oracle changed after preparation")
    require(oracle["fixture_sha256"] == record["fixture_sha256"], "fixture identity mismatch")
    require(isinstance(answers, list), "episode answers must be a list")
    require(len(json.dumps(answers).encode()) <= MAX_ANSWER_BYTES, "answers exceed evaluation bound")
    executions = _execution_evidence(execution_evidence, len(record["steps"]))
    answer_hash = fingerprint(answers)
    packet_id = fingerprint({"fixture_sha256": record["fixture_sha256"], "answer_sha256": answer_hash,
                             "execution_evidence_sha256": fingerprint(executions)})
    hard_errors, gaps, steps, evidence_by_step = [], [], [], []
    if len(answers) != len(record["steps"]):
        hard_errors.append(_issue(None, "incomplete-episode", "Every requested turn needs its own final answer."))
    for index, expected in enumerate(oracle["steps"]):
        answer = answers[index] if index < len(answers) else None
        row = {"step_id": index, "known_true_findings": [], "duplicate_findings": [],
               "unclassified_findings": [], "covered_obligations": [], "evidence": []}
        steps.append(row)
        direct_evidence = []
        evidence_by_step.append(direct_evidence)
        try:
            _validate_schema(answer, answer_schema())
        except (InvalidLedger, KeyError, TypeError) as error:
            hard_errors.append(_issue(index, "invalid-answer", str(error)))
            continue
        for evidence_index, item in enumerate(answer["evidence"]):
            try:
                direct_evidence.append(_evidence(item, oracle, record, index))
            except (InvalidLedger, KeyError, TypeError) as error:
                direct_evidence.append(None)
                hard_errors.append(_issue(index, "invalid-source-evidence", str(error), evidence_index=evidence_index))
        available = list(direct_evidence)
        for retained in answer["retained_evidence"]:
            try:
                origin, evidence_index = retained["step"], retained["index"]
                require(0 <= origin < index and 0 <= evidence_index < len(evidence_by_step[origin]),
                        "retained evidence references an unavailable earlier item")
                require(bool(retained["retention_proof"].strip()), "retention needs explicit public identity/change proof")
                item = evidence_by_step[origin][evidence_index]
                require(item is not None, "cannot retain invalid evidence")
                side = retained["side"]
                require(retained["snapshot"] == record["steps"][index][side + "_tree"],
                        "retained evidence snapshot differs from its current comparison side")
                current = _source(oracle, record, index, side, item["path"])
                require(hashlib.sha256(current.encode()).hexdigest() == item["source_sha256"],
                        "retained file content changed; refresh its source")
                available.append({**item, "retained": True, "side": side, "snapshot": retained["snapshot"],
                                  "origin_snapshot": item["snapshot"], "origin_side": item["side"],
                                  "retention_proof": retained["retention_proof"]})
            except (InvalidLedger, IndexError, KeyError, TypeError) as error:
                available.append(None)
                hard_errors.append(_issue(index, "invalid-retained-evidence", str(error)))
        row["evidence"] = [item for item in available if item is not None]
        if not row["evidence"]:
            hard_errors.append(_issue(index, "missing-source-evidence", "A review conclusion needs actual pinned evidence."))
        if answer["conclusion"] != expected["conclusion"]:
            hard_errors.append(_issue(index, "conclusion-mismatch", "The answer does not satisfy the frozen review task."))
        if not expected["defects"] and answer["findings"]:
            hard_errors.append(_issue(index, "clean-case-false-positive", "The in-scope comparison has no introduced defect."))
        seen_defects = set()
        for finding_index, finding in enumerate(answer["findings"]):
            try:
                _location(finding["location"], oracle, record, index)
                require(finding["cause"].strip() and finding["impact"].strip(), "finding needs cause and user impact")
                refs = finding["evidence_indices"]
                require(refs and all(type(ref) is int and 0 <= ref < len(available) and available[ref] is not None for ref in refs),
                        "finding references invalid or absent evidence")
                observed, witness_matches, contradiction = _witness(finding["trigger"], expected["defects"])
                require(not contradiction, "known-domain witness contradicts independent literal results")
                located = {defect["id"] for defect in expected["defects"]
                           if any(_overlaps(finding["location"], target) for target in defect["locations"])}
                matching = located & witness_matches
                if len(matching) == 1:
                    identity = next(iter(matching))
                    entry = {"finding_index": finding_index, "defect_id": identity, "trigger": observed}
                    row["duplicate_findings" if identity in seen_defects else "known_true_findings"].append(entry)
                    seen_defects.add(identity)
                else:
                    row["unclassified_findings"].append(finding_index)
                    gaps.append(_issue(index, "finding-needs-semantic-review", "No complete known location/witness match; adjudicate the actual claim.",
                                       gap_id=f"s{index}:finding:{finding_index}", finding_index=finding_index))
            except (InvalidLedger, ValueError, TypeError, KeyError) as error:
                hard_errors.append(_issue(index, "invalid-finding", str(error), finding_index=finding_index))
        for defect in expected["defects"]:
            if defect["id"] not in seen_defects:
                gaps.append(_issue(index, "defect-not-automatically-established", defect["rubric"],
                                   gap_id=f"s{index}:defect:{defect['id']}", defect_id=defect["id"]))
        for obligation in expected.get("evidence_obligations", []):
            covered = any(all(any(_covers(item, target, oracle, record, index) for item in row["evidence"])
                              for target in alternative) for alternative in obligation["alternatives"])
            if covered:
                row["covered_obligations"].append(obligation["id"])
            else:
                gaps.append(_issue(index, "evidence-obligation-needs-review", obligation["description"],
                                   gap_id=f"s{index}:evidence:{obligation['id']}", obligation_id=obligation["id"]))
    automatic = {"passed": not hard_errors and not gaps, "hard_errors": hard_errors, "manual_gaps": gaps,
                 "steps": steps, "source_delivery_observed": False,
                 "semantic_correctness_established": False, "validation_execution_verified": False}
    packet = {"schema": 1, "packet_id": packet_id, "answer_sha256": answer_hash,
              "case_id": record["case_id"], "fixture_sha256": record["fixture_sha256"],
              "tasks": [{key: step[key] for key in ("step_id", "task", "base_commit", "head_commit", "base_tree", "head_tree")}
                        for step in record["steps"]], "answers": answers, "automatic": automatic,
              "execution_evidence": executions, "execution_evidence_sha256": fingerprint(executions),
              "permitted_check_commands": record["check_commands"],
              "oracle_rubrics": oracle["steps"], "source_snapshots": oracle["snapshots"],
              "instructions": "Review cause, impact, actual binding, necessary evidence and asserted test execution. "
                              "Do not see arm identity, model cost or tool brand until this decision is final. "
                              "Exact source and literal witness matches do not establish causal prose. "
                              "Execution records contain observed command output, not a guarantee of complete raw output. "
                              "Check actual command semantics: echoing a command or mentioning a test is not executing it. "
                              "Alternative valid evidence/witnesses require explicit gap resolutions with reasons."}
    unresolved = list(gaps)
    regressions = list(hard_errors)
    status = "pending"
    if adjudication is not None:
        unresolved, semantic_errors = _adjudicate(adjudication, packet, answers)
        regressions.extend(semantic_errors)
        status = "accepted"
    missing = unresolved + ([] if adjudication is not None else ["blinded-semantic-adjudication-pending"])
    quality = {"passed": adjudication is not None and not regressions and not missing,
               "regressions": regressions, "missing_evidence": missing, "evidence_sha256": answer_hash}
    return {"schema": 1, "case_id": record["case_id"], "packet_id": packet_id, "answer_sha256": answer_hash,
            "automatic": automatic, "adjudication_status": status, "adjudication_packet": packet,
            "validation_claims_checked": adjudication is not None,
            "validation_execution_verified": adjudication is not None and bool(executions)
                and any(row["execution_claims"] == "supported" for row in adjudication["steps"])
                and all(row["execution_claims"] in ("none", "supported") for row in adjudication["steps"]),
            "quality": quality}


def _adjudicate(decision, packet, answers):
    require(isinstance(decision, dict) and decision.get("schema") == 1, "invalid adjudication schema")
    require(decision.get("packet_id") == packet["packet_id"] and decision.get("answer_sha256") == packet["answer_sha256"],
            "adjudication is not bound to these answers")
    require(decision.get("blinded") is True and isinstance(decision.get("reviewer"), str)
            and decision["reviewer"].strip(), "blinded reviewer identity required")
    rows = decision.get("steps")
    require(isinstance(rows, list) and all(isinstance(row, dict) for row in rows)
            and [row.get("step_id") for row in rows] == list(range(len(packet["tasks"]))),
            "adjudication must cover every episode step in order")
    gaps = {gap["gap_id"]: gap for gap in packet["automatic"]["manual_gaps"]}
    resolved, failures = set(), []
    for row in rows:
        index = row["step_id"]
        require(type(row.get("semantic_passed")) is bool and isinstance(row.get("notes"), str)
                and row["notes"].strip(), "semantic verdict and explanation required")
        for field in ("unsupported_finding_indices", "missed_defect_ids", "missing_evidence"):
            require(isinstance(row.get(field), list), "missing adjudication " + field)
        claim_kind = row.get("execution_claims")
        require(claim_kind in ("none", "supported", "unsupported"), "explicit test-execution claim assessment required")
        execution_refs = row.get("execution_evidence_indices")
        require(isinstance(execution_refs, list) and all(type(ref) is int and 0 <= ref < len(packet["execution_evidence"])
                    and packet["execution_evidence"][ref]["step_id"] == index for ref in execution_refs),
                "invalid fixture execution evidence reference")
        require(claim_kind != "supported" or execution_refs,
                "cannot verify execution claims without controller-observed fixture-check evidence")
        if claim_kind == "unsupported":
            failures.append(_issue(index, "unsupported-test-execution-claim", row["notes"]))
        answer = answers[index] if index < len(answers) and isinstance(answers[index], dict) else {}
        findings = answer.get("findings", [])
        finding_count = len(findings) if isinstance(findings, list) else 0
        require(all(type(value) is int and 0 <= value < finding_count
                    for value in row["unsupported_finding_indices"]), "unknown adjudicated finding index")
        if not row["semantic_passed"] or any(row[field] for field in (
                "unsupported_finding_indices", "missed_defect_ids", "missing_evidence")):
            failures.append(_issue(index, "semantic-review-failed", row["notes"],
                                   unsupported_finding_indices=row["unsupported_finding_indices"],
                                   missed_defect_ids=row["missed_defect_ids"], missing_evidence=row["missing_evidence"]))
        resolutions = row.get("resolutions", [])
        require(isinstance(resolutions, list) and all(isinstance(item, dict) for item in resolutions),
                "adjudication resolutions must be objects")
        for resolution in resolutions:
            gap_id = resolution.get("gap_id")
            require(gap_id in gaps and gaps[gap_id]["step_id"] == index and gap_id not in resolved,
                    "unknown or repeated adjudication gap resolution")
            require(isinstance(resolution.get("reason"), str) and resolution["reason"].strip(),
                    "alternative evidence needs an explicit reason")
            require(isinstance(resolution.get("evidence_indices"), list) and resolution["evidence_indices"]
                    and all(type(ref) is int and 0 <= ref < len(packet["automatic"]["steps"][index]["evidence"])
                            for ref in resolution["evidence_indices"]), "alternative evidence references required")
            resolved.add(gap_id)
    return [gap for identity, gap in gaps.items() if identity not in resolved], failures
