"""Validate and total canonical provider-call usage without inferring missing measurements."""

import argparse
import hashlib
import json
from collections import defaultdict
from decimal import Decimal, InvalidOperation
from pathlib import Path

SCHEMA = 1
MAX_BYTES = 16 * 1024 * 1024
MAX_EVENTS = 100_000
CONDITIONS = (
    "task_id", "fixture_sha256", "task_sha256", "oracle_sha256", "model",
    "harness", "start_context_sha256", "mode", "provider", "retention_policy", "evidence_kind",
)
TOKEN_FIELDS = (
    "input_tokens", "input_uncached_tokens", "input_cache_read_tokens",
    "input_cache_creation_tokens", "output_tokens",
)


class InvalidLedger(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise InvalidLedger(message)


def integer(value, name):
    require(type(value) is int and value >= 0, f"{name}: expected nonnegative integer")
    return value


def text(value, name):
    require(isinstance(value, str) and 0 < len(value) <= 4096, f"{name}: missing or oversized string")
    return value


def sha256(value, name):
    text(value, name)
    require(len(value) == 64 and all(char in "0123456789abcdef" for char in value), f"{name}: expected lowercase SHA256")
    return value


def object_pairs(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, f"duplicate JSON field: {key}")
        result[key] = value
    return result


def fingerprint(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def union_size(ranges):
    end = -1
    size = 0
    for start, stop in sorted(ranges):
        size += max(0, stop - max(start, end))
        end = max(end, stop)
    return size


def summarize(manifest, events):
    require(manifest.get("schema") == SCHEMA, "unsupported manifest schema")
    require(manifest.get("variant") in ("baseline", "reposcout"), "invalid variant")
    require(manifest.get("mode") in ("isolated", "end-to-end"), "invalid mode")
    require(manifest.get("evidence_kind") in ("synthetic", "observed"), "evidence kind required")
    for field in CONDITIONS + ("run_id",):
        text(manifest.get(field), field)
    for field in ("fixture_sha256", "task_sha256", "oracle_sha256", "start_context_sha256"):
        sha256(manifest[field], field)
    seen = set()
    calls = set()
    agents = {}
    attempts = set()
    usage = dict.fromkeys(TOKEN_FIELDS, 0)
    costs = defaultdict(Decimal)
    costs_reported = 0
    reasoning = 0
    reasoning_known = 0
    source_ranges = defaultdict(list)
    source_bytes = 0
    source_reads = 0
    followup_reads = 0
    tool_bytes = 0
    tool_calls = 0
    latencies = defaultdict(list)
    context_events = []
    quality = None
    end = None
    count = 0
    for event in events:
        count += 1
        require(count <= MAX_EVENTS, "too many events")
        require(end is None, "event after run_end")
        require(isinstance(event, dict), "event must be object")
        require(event.get("schema") == SCHEMA, "unsupported event schema")
        require(event.get("run_id") == manifest["run_id"], "foreign run event")
        identity = text(event.get("event_id"), "event_id")
        require(identity not in seen, f"duplicate or conflicting event_id: {identity}")
        seen.add(identity)
        kind = event.get("kind")
        if kind == "agent":
            agent = text(event.get("agent_id"), "agent_id")
            require(agent not in agents, "duplicate agent registration")
            parent = event.get("parent_agent_id")
            require(parent is None or parent in agents, "unknown parent agent")
            agents[agent] = parent
            continue
        if kind in ("usage", "tool", "source", "context"):
            agent = event.get("agent_id")
            require(agent in agents, "unregistered agent")
            attempt = text(event.get("attempt_id"), "attempt_id")
            attempts.add((agent, attempt))
        if kind == "usage":
            require(event.get("provider") == manifest["provider"], "provider differs from scenario")
            require(event.get("model") == manifest["model"], "model differs from scenario")
            call = (event["provider"], text(event.get("provider_call_id"), "provider_call_id"))
            require(call not in calls, "duplicate or conflicting provider call")
            calls.add(call)
            require(event.get("final") is True, "streaming/cumulative usage must first normalize to one final call")
            require(event.get("status") in ("success", "failed", "aborted"), "invalid call status")
            provenance = event.get("provenance", {})
            for field in ("raw_sha256", "mapping_id", "mapping_reference"):
                text(provenance.get(field), "provenance." + field)
            sha256(provenance["raw_sha256"], "raw_sha256")
            values = {field: integer(event.get(field), field) for field in TOKEN_FIELDS}
            require(values["input_tokens"] == sum(values[field] for field in TOKEN_FIELDS[1:4]), "input partition double-counts or omits tokens")
            for field, value in values.items():
                usage[field] += value
            if event.get("reasoning_tokens") is not None:
                value = integer(event["reasoning_tokens"], "reasoning_tokens")
                require(value <= values["output_tokens"], "reasoning must be a subset of output_tokens")
                reasoning += value
                reasoning_known += 1
            if event.get("provider_cost") is not None:
                cost = event["provider_cost"]
                currency = text(cost.get("currency"), "currency")
                require(isinstance(cost.get("amount"), str), "cost amount must be a decimal string")
                try:
                    amount = Decimal(cost["amount"])
                except (InvalidOperation, KeyError, TypeError):
                    raise InvalidLedger("invalid provider cost") from None
                require(amount.is_finite() and amount >= 0, "invalid provider cost")
                costs[currency] += amount
                costs_reported += 1
        elif kind == "tool":
            tool_calls += 1
            tool_bytes += integer(event.get("output_bytes"), "output_bytes")
            phase = event.get("phase")
            require(phase in ("cold", "warm", "after-edit", "other"), "invalid latency phase")
            latencies[phase].append(integer(event.get("latency_ms"), "latency_ms"))
        elif kind == "source":
            path = text(event.get("path"), "source.path")
            snapshot = sha256(event.get("source_sha256"), "source_sha256")
            start = integer(event.get("start_byte"), "start_byte")
            stop = integer(event.get("end_byte"), "end_byte")
            require(start <= stop, "reversed source span")
            require(type(event.get("followup")) is bool, "followup must be explicit")
            source_ranges[(path, snapshot)].append((start, stop))
            source_bytes += stop - start
            source_reads += 1
            followup_reads += event["followup"]
        elif kind == "context":
            require(event.get("operation") in ("retention", "compaction"), "invalid context operation")
            context_events.append(event)
        elif kind == "quality":
            require(quality is None, "duplicate quality record")
            require(type(event.get("passed")) is bool, "quality.passed must be explicit")
            for field in ("regressions", "missing_evidence"):
                require(isinstance(event.get(field), list), f"quality.{field} must be list")
            sha256(event.get("evidence_sha256"), "quality evidence_sha256")
            quality = event
        elif kind == "run_end":
            require(event.get("status") in ("success", "failed", "aborted"), "invalid run status")
            require(type(event.get("usage_complete")) is bool, "usage completeness required")
            declared_agents = event.get("agent_ids")
            declared_calls = event.get("provider_call_ids")
            require(isinstance(declared_agents, list) and len(declared_agents) == len(set(declared_agents)) and set(declared_agents) == set(agents), "agent inventory differs from ledger")
            require(isinstance(declared_calls, list) and len(declared_calls) == len(set(declared_calls)) and set(declared_calls) == {call[1] for call in calls}, "call inventory differs from ledger")
            end = event
        else:
            raise InvalidLedger(f"unknown event kind: {kind}")
    require(end is not None, "missing run_end (record aborted status explicitly)")
    require(quality is not None, "quality evidence required even for failed/aborted runs")
    require(len([parent for parent in agents.values() if parent is None]) == 1, "exactly one root agent required")
    require(end["status"] != "success" or quality["passed"], "successful run requires passing quality")
    require(end["status"] != "success" or len(calls) > 0, "successful model run requires actual call records")
    unique_source = sum(union_size(ranges) for ranges in source_ranges.values())
    return {
        "schema": SCHEMA, "run_id": manifest["run_id"], "variant": manifest["variant"],
        "conditions": {field: manifest[field] for field in CONDITIONS},
        "status": end["status"], "usage_complete": end["usage_complete"], "quality": quality,
        "model_tokens": {**usage, "total_tokens": usage["input_tokens"] + usage["output_tokens"],
                         "reasoning_tokens_subset": reasoning, "reasoning_reported_calls": reasoning_known},
        "agents": len(agents), "attempts": len(attempts), "model_calls": len(calls),
        "provider_cost": {"amounts": {key: str(value) for key, value in sorted(costs.items())},
                          "reported_calls": costs_reported},
        "explanatory": {"tool_calls": tool_calls, "tool_output_bytes": tool_bytes,
                        "source_reads": source_reads, "followup_reads": followup_reads,
                        "source_bytes": source_bytes, "unique_source_bytes": unique_source,
                        "duplicate_source_bytes": source_bytes - unique_source,
                        "latency_ms": dict(latencies), "context_events": context_events},
    }


def compare(baseline, candidate):
    require(baseline["variant"] == "baseline" and candidate["variant"] == "reposcout", "comparison variants invalid")
    require(baseline["conditions"] == candidate["conditions"], "incomparable scenarios")
    eligible = all(result["conditions"]["evidence_kind"] == "observed"
                   and result["usage_complete"] and result["status"] == "success"
                   and result["quality"]["passed"] and not result["quality"]["regressions"]
                   and not result["quality"]["missing_evidence"] for result in (baseline, candidate))
    return {"schema": SCHEMA, "conditions": baseline["conditions"], "eligible": eligible,
            "baseline": baseline, "reposcout": candidate,
            "token_delta": candidate["model_tokens"]["total_tokens"] - baseline["model_tokens"]["total_tokens"] if eligible else None}


def read_json(path):
    with Path(path).open("rb") as stream:
        data = stream.read(MAX_BYTES + 1)
    require(len(data) <= MAX_BYTES, "input exceeds ledger limit")
    return json.loads(data, object_pairs_hook=object_pairs)


def read_events(path):
    with Path(path).open("rb") as stream:
        total = 0
        while line := stream.readline(MAX_BYTES + 1):
            total += len(line)
            require(total <= MAX_BYTES, "events exceed ledger limit")
            if line.strip():
                yield json.loads(line, object_pairs_hook=object_pairs)


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    account = sub.add_parser("summarize")
    account.add_argument("manifest")
    account.add_argument("events")
    comparison = sub.add_parser("compare")
    comparison.add_argument("baseline")
    comparison.add_argument("reposcout")
    args = parser.parse_args()
    try:
        result = summarize(read_json(args.manifest), read_events(args.events)) if args.command == "summarize" else compare(read_json(args.baseline), read_json(args.reposcout))
        print(json.dumps(result, sort_keys=True, indent=2))
    except (InvalidLedger, ValueError, OSError, KeyError, TypeError) as error:
        parser.exit(2, f"{error}\n")


if __name__ == "__main__":
    main()
