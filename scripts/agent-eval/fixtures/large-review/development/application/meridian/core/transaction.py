from contextlib import contextmanager
from copy import deepcopy


@contextmanager
def atomic(state):
    snapshot = deepcopy(vars(state))
    try:
        yield
    except Exception:
        vars(state).clear()
        vars(state).update(snapshot)
        raise
