"""Closed, test-only import observations. No resolver, package imports or timers."""
import json
import sys
import time

IMPORTS = ('duckdb', 'networkx', 'spacy', 'click', 'splink', 'pyarrow')
ATTEMPTS = frozenset(('numpy', 'numpy._core._multiarray_umath', 'catalogue', 'confection',
    'thinc', 'thinc.compat', 'thinc.backends.numpy_ops', 'blis', 'blis.cy', 'srsly',
    'pydantic_core', 'pydantic_core._pydantic_core', 'spacy.pipeline', 'spacy.language',
    'spacy.cli', 'weasel'))
MAX_RECORD = 512
MAX_CLOCK_MS = 120_000  # Acceptance bound only; does not extend the 30-second worker limit.


def require(ok):
    if not ok:
        raise ValueError('fixed-import-diagnostic-contract')


def write_record(path, record):
    raw = json.dumps(record, sort_keys=True, allow_nan=False).encode()
    require(len(raw) <= MAX_RECORD)
    with path.open('xb') as stream:
        stream.write(raw)


class ImportDiagnostics:
    def __init__(self, scratch, *, clock=time.monotonic_ns, cpu=time.process_time_ns, writer=write_record):
        self.scratch, self.clock, self.cpu, self.writer = scratch, clock, cpu, writer
        self.started, self.cpu_started = clock(), cpu()
        self.previous = (0, 0)
        self.top_count = 0
        self.seen = set()
        self.active = False
        self.installed = False
        self.busy = False
        self.failed = False

    def stamp(self):
        values = ((self.clock() - self.started) // 1_000_000,
                  (self.cpu() - self.cpu_started) // 1_000_000)
        require(all(type(value) is int and old <= value <= MAX_CLOCK_MS
                    for old, value in zip(self.previous, values)))
        self.previous = values
        return dict(zip(('elapsed_ms', 'process_cpu_ms'), values))

    def checkpoint(self, module, boundary):
        index = self.top_count
        require(index < 12 and module == IMPORTS[index // 2]
                and boundary == ('before', 'after')[index % 2])
        self.writer(self.scratch / f'import-{index}.json',
                    {'module': module, 'boundary': boundary, **self.stamp()})
        self.top_count += 1

    def install(self):
        require(not self.installed)
        self.installed = True
        self.active = True
        sys.addaudithook(self.observe)

    def observe(self, event, args):
        # Constant-size filtering. Never stringify event arguments or copy paths.
        # Reentrant audit callbacks from our own file writes are ignored.
        if (not self.active or self.busy or type(event) is not str or event != 'import'
                or type(args) is not tuple or not args or type(args[0]) is not str or len(args[0]) > 64):
            return
        module = args[0]
        if module not in ATTEMPTS or module in self.seen:
            return
        self.busy = True
        try:
            ordinal = len(self.seen)
            require(ordinal < 16)
            self.seen.add(module)
            self.writer(self.scratch / f'import-attempt-{ordinal}.json',
                        {'module': module, 'ordinal': ordinal, **self.stamp()})
            if len(self.seen) == 16:
                self.active = False
        except Exception:
            # Diagnostic I/O must not replace an exception from the real import.
            # A completed recipe still refuses success in finish(). A killed
            # recipe retains only the independently validated partial records.
            self.failed = True
            self.active = False
        finally:
            self.busy = False

    def finish(self):
        self.active = False
        require(self.installed and not self.failed and self.top_count == 12)
