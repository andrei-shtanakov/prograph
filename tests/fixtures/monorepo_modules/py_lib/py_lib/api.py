"""Public surface module."""

from py_lib.helpers import normalize
from py_lib.storage import Store


class PublicAPI:
    """Top-level public class."""

    def __init__(self):
        self.store = Store()

    def query(self, q):
        return normalize(q)


def public_fn(x):
    return x * 2


def _private_helper():
    return 42


PUBLIC_CONST = "v1"
