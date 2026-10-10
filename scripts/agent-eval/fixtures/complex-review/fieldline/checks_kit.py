"""Assembly helpers shared by the unit and API test generators."""

from collections import namedtuple

from .core import imports, render

NODE = "import assert from 'node:assert/strict';\n"
DESCRIBE_IT = "import { describe, it } from 'node:test';\n"
WITH_SETUP = "import { beforeEach, describe, it } from 'node:test';\n"

# groups: import targets (a dict or callable(ctx)); body: template text or callable(ctx); jobs: work-order symbols;
# subs: marker values as a dict or callable(ctx); node: the node:test import line.
Spec = namedtuple("Spec", "groups body jobs subs node", defaults=((), None, DESCRIBE_IT))


def test_file(ctx, path, spec):
    """A complete test file: node imports, project imports, then the rendered body."""
    body = spec.body(ctx) if callable(spec.body) else spec.body
    subs = spec.subs(ctx) if callable(spec.subs) else (spec.subs or {})
    groups = spec.groups(ctx) if callable(spec.groups) else spec.groups
    head = NODE + spec.node + imports(ctx, path, groups, jobs=spec.jobs) + "\n"
    return head + render(ctx, body, **subs)


def register(ctx, files):
    for path, spec in files.items():
        ctx.add(path, test_file(ctx, path, spec))


APP = {"src/app.ts": ["createApp"]}
WORLD = "tests/support/world.ts"
