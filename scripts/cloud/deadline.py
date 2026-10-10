"""A request deadline scoped to one recovery thread, shared by provider leaves."""
import contextlib
import contextvars
import time

_current = contextvars.ContextVar('cloud_request_deadline', default=None)


def active():
    return _current.get() is not None


@contextlib.contextmanager
def bound(until, *, now=None):
    """Nested bounds may shorten an existing absolute monotonic deadline."""
    previous = _current.get()
    if previous is not None:
        until = min(until, previous[0])
    token = _current.set((until, time.monotonic if now is None else now))
    try:
        yield
    finally:
        _current.reset(token)


def remaining(cap=120):
    """Positive seconds a request may spend; expired contexts start no I/O."""
    state = _current.get()
    if state is None:
        return cap
    left = state[0] - state[1]()
    if left <= 0:
        raise TimeoutError('cloud recovery request deadline expired')
    return min(cap, left)


class Client:
    """Rebuild an SDK client with the remaining request budget for each call."""
    def __init__(self, factory):
        self.factory = factory

    def __getattr__(self, method):
        def call(*args, **kwargs):
            client = self.factory(remaining())
            return getattr(client, method)(*args, **kwargs)
        return call
