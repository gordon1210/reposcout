from meridian.config import default_settings
from meridian.core.state import State
from meridian.api.dispatch import dispatch


class Application:
    def __init__(self, settings=None):
        self.settings = settings or default_settings()
        self.state = State()

    def request(self, method, path, credential, body=None):
        return dispatch(self, method, path, credential, body)
