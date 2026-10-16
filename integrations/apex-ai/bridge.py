#!/usr/bin/env python3
"""Path-independent entry to the existing external Apex analysis bridge."""
from contextlib import redirect_stdout
import argparse
import importlib.util
import json
import os
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]


def backend_root(env):
    return Path(env.get('APEX_SERVICE_ROOT') or ROOT.parent / 'apex-fps-mvp').expanduser().resolve()


def load_backend(root):
    entry = root / 'integrations/filmcraft/bridge.py'
    if not entry.is_file():
        raise FileNotFoundError('共享分析入口不存在：' + str(entry) + '；请设置 APEX_SERVICE_ROOT')
    if str(root) not in sys.path:
        sys.path.insert(0, str(root))
    spec = importlib.util.spec_from_file_location('_filmcraft_apex_backend', entry)
    if spec is None or spec.loader is None:
        raise ImportError('无法载入共享分析入口：' + str(entry))
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    with redirect_stdout(sys.stderr):
        spec.loader.exec_module(module)
    return module


class CategorizedOutput:
    """Forward progress immediately, defer the result until core cleanup finishes."""
    def __init__(self, output):
        self.output, self.pending, self.buffer = output, [], ''

    def write(self, data):
        self.buffer += data
        while '\n' in self.buffer:
            line, self.buffer = self.buffer.split('\n', 1)
            if not line.strip():
                continue
            event = json.loads(line)
            if event.get('type') == 'result':
                self.pending.append(event)
            else:
                self.output.write(line + '\n')
                self.output.flush()

    def flush(self):
        self.output.flush()


def main(argv=None):
    env = dict(os.environ)
    env.setdefault('APEX_AI_DATA_DIR', str(ROOT / 'outputs/apex-ai/model-data'))
    env.setdefault('APEX_MODEL_WORKSPACE', str(ROOT.parent / 'apex-voice-detect'))
    try:
        module = load_backend(backend_root(env))
        args = sys.argv[1:] if argv is None else argv
        if args and args[0] == 'validate':
            from recovery import main as validate
            return validate(module, args, env, sys.stdout)
        if not args or args[0] != 'analyze':
            return module.main(args, env=env)
        output = CategorizedOutput(sys.stdout)
        # The backend releases its worker and directory lock before a second
        # service is opened. No globals are patched and no new model is loaded.
        with redirect_stdout(sys.stderr):
            status = module.main(args, env=env, output=output)
        if status != 0:
            return status
        if output.buffer.strip() or len(output.pending) != 1:
            raise ValueError('共享分析没有返回唯一完整结果')
        from voice import categorized_result
        parser = argparse.ArgumentParser(add_help=False)
        parser.add_argument('--parent-pid', type=int, default=os.getppid())
        parent, _ = parser.parse_known_args(args)
        service = None
        with module.ParentGuard(parent.parent_pid) as guard:
            try:
                with redirect_stdout(sys.stderr):
                    config = module.configuration(env, analyze=True)
                    service = module.ModelService(**config.service_arguments())
                    event = output.pending[0]
                    event['result'] = categorized_result(service, event['result'], guard.check)
                guard.check()
                print(json.dumps(event, ensure_ascii=False, allow_nan=False), flush=True)
            finally:
                if service is not None:
                    with redirect_stdout(sys.stderr):
                        service.close(interrupted=getattr(guard, 'reason', None) == 'service_interrupted')
        return 0
    except FileNotFoundError as exc:
        code, message = 'backend_unavailable', str(exc)
    except ImportError as exc:
        code, message = 'backend_import_failed', str(exc)
    except Exception as exc:
        code, message = getattr(exc, 'code', 'backend_failed'), str(exc)
    print(json.dumps({'type': 'error', 'code': code, 'message': message}, ensure_ascii=False), flush=True)
    return 1


if __name__ == '__main__':
    raise SystemExit(main())
