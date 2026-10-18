#!/usr/bin/env python3
"""PyInstaller entry: public bridge plus supervised worker re-entry modes."""
import json
import os
import sys


def main():
    arguments = sys.argv[1:]
    try:
        if arguments[:1] == ['--process-guardian']:
            from model_runtime import guardian_main
            return guardian_main(arguments[1:])
        if arguments[:1] == ['--inference-worker']:
            from runtime import runtime_environment
            runtime_environment(dict(os.environ))
            from model_inference import main as worker_main
            return worker_main() or 0
        from bridge import main as bridge_main
        return bridge_main(arguments)
    except Exception as exc:
        print(json.dumps({'type': 'error', 'code': getattr(exc, 'code', 'runtime_unavailable'),
                          'message': str(exc)}, ensure_ascii=False), flush=True)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
