"""Read-only validation of one saved FilmCraft association through the shared core."""
import re
import sys
import json
import zipfile
from pathlib import Path


class AssociationError(Exception):
    code = 'association_invalid'


def validate_association(service, source, audio, identifier, check):
    if not re.fullmatch(r'[0-9a-f]{32}', identifier):
        raise AssociationError('分析关联编号无效，请主动重试分析')
    folder = service.results_dir / identifier
    if folder.is_symlink() or (folder/'result.json').is_symlink() or folder.resolve().parent != service.results_dir.resolve():
        raise AssociationError('分析关联路径无效；请主动重试')
    try:
        result = service.execute('get_result', {'result_id': identifier})
    except Exception as exc:
        if isinstance(exc, (OSError, ValueError)) or getattr(exc, 'code', None) == 'result_not_found':
            raise AssociationError('保存的分析关联记录缺失或损坏；已有人工工作仍保留，请主动重试：' + str(exc)) from exc
        raise
    if (not isinstance(result, dict) or not isinstance(result.get('media'), dict)
            or not isinstance(result['media'].get('source'), dict)
            or not isinstance(result['media']['source'].get('path'), str)
            or not isinstance(result['media'].get('selected_audio'), dict)
            or not isinstance(result.get('prediction'), dict)
            or not all(isinstance(result.get(key), list) for key in ('events', 'candidates'))):
        raise AssociationError('保存的分析关联结构损坏；已有人工工作仍保留，请主动重试')
    media = result['media']
    if (result.get('result_id') != identifier or result.get('prediction_id') != identifier
            or Path(media['source']['path']).resolve() != source.resolve()
            or media['selected_audio']['stream_index'] != audio):
        raise AssociationError('保存的分析关联与原录像或分析音轨不一致')
    complete = False
    # Batch result IDs need not equal their parent task ID. Inspect only task
    # linkage here; prediction identity/validity stays owned by the shared core.
    for task_path in service.tasks_dir.glob('*.json'):
        check()
        if task_path.is_symlink() or not re.fullmatch(r'[0-9a-f]{32}', task_path.stem):
            continue
        try:
            task = service.execute('get_task', {'task_id': task_path.stem})
        except (OSError, ValueError, KeyError, TypeError):
            continue
        if (task.get('kind') == 'analysis' and task.get('status') == 'completed'
                and any(r.get('status') == 'completed' and r.get('result_id') == identifier
                        and r.get('media_id') == media.get('media_id') for r in task.get('results', []))):
            complete = True
            break
    if not complete:
        raise AssociationError('保存的分析没有完整的完成任务记录')
    # FilmCraft association integrity: paths are derived from the admitted ID,
    # never opened from untrusted saved scores_path strings.
    folder = service.results_dir / identifier
    scores, metadata = folder / 'scores.npz', folder / 'prediction.json'
    prediction = result.get('prediction', {})
    try:
        if (folder.is_symlink() or any(p.is_symlink() for p in (folder/'result.json', scores, metadata))
                or folder.resolve().parent != service.results_dir.resolve()
                or Path(prediction.get('scores_path', '')).resolve() != scores.resolve()
                or Path(prediction.get('metadata_path', '')).resolve() != metadata.resolve()):
            raise ValueError('结果文件位置不匹配')
        saved = json.loads(metadata.read_text(encoding='utf-8'))
        if (saved.get('prediction_id') != identifier or saved.get('media') != media
                or saved.get('configuration') != prediction.get('configuration')):
            raise ValueError('预测元数据身份不匹配')
        with zipfile.ZipFile(scores) as archive:
            required = {'probabilities.npy','class_names.npy','frame_onsets.npy','frame_offsets.npy','window_counts.npy'}
            if not required.issubset(archive.namelist()) or archive.testzip() is not None:
                raise ValueError('完整分数缺失或损坏')
    except (OSError, ValueError, TypeError, KeyError, zipfile.BadZipFile) as exc:
        raise AssociationError('分析关联文件缺失或损坏；已保存的事件和人工工作仍保留，请主动重试：' + str(exc)) from exc
    # get_result only reads JSON. The public validator checks source content,
    # selected audio and current model/configuration; score paging checks data.
    from voice import categorized_result
    categorized_result(service, result, check)
    check()
    return {'type': 'validation', 'status': 'valid', 'result_id': identifier,
            'prediction_id': identifier, 'source': media['source'], 'audio_stream': audio}


def main(module, argv, env, output):
    import argparse
    import os
    from contextlib import redirect_stdout
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['validate'])
    parser.add_argument('--input', required=True)
    parser.add_argument('--audio-stream', required=True, type=int)
    parser.add_argument('--result-id', required=True)
    parser.add_argument('--parent-pid', type=int, default=os.getppid())
    args = parser.parse_args(argv)
    source = Path(args.input).expanduser()
    if not source.is_absolute() or not source.is_file():
        raise AssociationError('原片离线或已移动，已保存的识别与剪辑仍保留')
    if args.parent_pid <= 0 or args.parent_pid == os.getpid():
        raise AssociationError('宿主进程无效')
    service = None
    with module.ParentGuard(args.parent_pid) as guard:
        try:
            with redirect_stdout(sys.stderr):
                config = module.configuration(env, analyze=True)
                service = module.ModelService(**config.service_arguments())
                if service.read_only:
                    raise module.BridgeError('data_dir_locked', 'AI 数据目录已由另一个分析任务使用；请稍后核对')
                event = validate_association(service, source, args.audio_stream, args.result_id, guard.check)
            output.write(json.dumps(event, ensure_ascii=False, allow_nan=False) + '\n')
            output.flush()
        finally:
            if service is not None:
                with redirect_stdout(sys.stderr):
                    service.close(interrupted=getattr(guard, 'reason', None) == 'service_interrupted')
    return 0
