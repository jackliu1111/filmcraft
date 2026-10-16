#!/usr/bin/env python3
"""Path-independent entry to the existing external Apex analysis bridge."""
from contextlib import redirect_stdout
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


def main(argv=None):
    env = dict(os.environ)
    env.setdefault('APEX_AI_DATA_DIR', str(ROOT / 'outputs/apex-ai/model-data'))
    env.setdefault('APEX_MODEL_WORKSPACE', str(ROOT.parent / 'apex-voice-detect'))
    try:
        module = load_backend(backend_root(env))
        return module.main(sys.argv[1:] if argv is None else argv, env=env)
    except FileNotFoundError as exc:
        code, message = 'backend_unavailable', str(exc)
    except ImportError as exc:
        code, message = 'backend_import_failed', str(exc)
    except Exception as exc:
        code, message = 'backend_failed', str(exc)
    print(json.dumps({'type': 'error', 'code': code, 'message': message}, ensure_ascii=False), flush=True)
    return 1


if __name__ == '__main__':
    raise SystemExit(main())
