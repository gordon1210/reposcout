"""Deterministic generator for the Fieldline field-service application and its variants."""

from . import (
    accounts,
    api,
    billing,
    checks_accounts,
    checks_api,
    checks_billing,
    checks_foundation,
    checks_integrations,
    checks_notifications,
    checks_operations,
    checks_quotes,
    checks_reports,
    checks_security,
    checks_shared,
    checks_sla,
    checks_workforce,
    checks_workorders,
    checks_worktypes,
    cleanup,
    docs,
    engagement,
    holdout_b_c,
    holdout_b_d,
    holdout_c_e,
    holdout_c_f,
    insights,
    operations,
    platform,
    servicelevels,
    shared,
    shift_rollout,
    testkit,
    workforce,
    workorders,
    worktypes,
)
from .core import VARIANTS, Ctx

MODULES = (
    shared, platform, accounts, workorders, worktypes, workforce, operations, billing, servicelevels,
    engagement, insights, api, testkit, checks_api, checks_shared, checks_foundation, checks_accounts,
    checks_workorders, checks_worktypes, checks_workforce, checks_operations, checks_billing, checks_sla,
    checks_quotes, checks_notifications,
    checks_reports, checks_integrations, checks_security, shift_rollout, cleanup, docs, holdout_b_c,
    holdout_b_d, holdout_c_e, holdout_c_f,
)


def build(variant):
    """Return `{relative path: text}` for one variant of the application."""
    ctx = Ctx(variant)
    for module in MODULES:
        module.generate(ctx)
    return dict(sorted(ctx.files.items()))


__all__ = ["VARIANTS", "build"]
