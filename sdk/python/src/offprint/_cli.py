import signal
import sys

from ._native import run_cli


def main() -> int:
    # Let the Rust command handle cancellation before Python checks pending signals.
    signal.signal(signal.SIGINT, signal.SIG_DFL)
    return run_cli(["offprint", *sys.argv[1:]])
