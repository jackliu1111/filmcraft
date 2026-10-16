"""FilmCraft's versioned spoken-voice policy over the shared prediction API.

This is postprocessing, not a second model implementation. Times returned by
read_scores are already in source-video coordinates and must not shift again.
"""
import math
import struct

SPOKEN_CLASSES = (
    'Speech', 'Male speech, man speaking', 'Female speech, woman speaking',
    'Child speech, kid speaking', 'Conversation', 'Narration, monologue',
    'Whispering', 'Speech synthesizer', 'Shout', 'Children shouting',
)
# Match the shared core's float32 ndarray comparison after JSON promotes
# stored values to Python doubles (float32(0.2) is slightly greater than 0.2).
THRESHOLD = struct.unpack('f', struct.pack('f', .2))[0]
POLICY = {
    'version': 'filmcraft-spoken-voice-v1', 'owner': 'FilmCraft adaptation',
    'classes': list(SPOKEN_CLASSES), 'threshold': .2, 'comparison': '>',
    'threshold_precision': 'float32-like-predictions',
    'median_frames': 3, 'median_padding': 'symmetric',
    'combine': 'union', 'events': 'contiguous-active-frames',
    'candidate_padding_s': 1., 'candidate_merge': 'none',
    'time_mapping': 'read_scores-source-video-seconds-no-additional-shift',
    'meaning': '所选分析音轨中的人声候选；不表示身份、转写或分离声部',
}


def number(value):
    return type(value) in (int, float) and math.isfinite(value)


def categorized_result(service, result, check_cancelled):
    """Consume only records validated against current source/audio/model identity."""
    service.validate_result(result)
    identifier = result['prediction_id']
    stored = service.execute('get_result', {'result_id': identifier})
    if (stored.get('prediction_id') != identifier or stored['media'] != result['media']
            or stored['prediction'] != result['prediction']):
        raise ValueError('完整预测与返回的来源、分析音轨或模型身份不一致')
    voice = {'status': 'unavailable', 'postprocess': POLICY, 'events': [], 'candidates': []}
    # Preserve the legacy top-level gun events/candidates, including their IDs.
    categories = {'gunshot': {'status': 'completed', 'events': result['events'],
                             'candidates': result['candidates']}, 'voice': voice}
    duration = result['media']['duration_s']
    if not number(duration) or duration <= 0:
        raise ValueError('原录像时长无效')
    starts, ends, scores, classes = [], [], [], None
    cursor = 0.
    while True:
        check_cancelled()
        page = service.execute('read_scores', {'prediction_id': identifier, 'start_s': cursor, 'end_s': duration})
        if page.get('prediction_id') != identifier:
            raise ValueError('完整分数的预测身份不匹配')
        names = page.get('classes')
        if (not isinstance(names, list) or not all(isinstance(c, str) for c in names)
                or len(set(names)) != len(names) or (classes is not None and names != classes)):
            raise ValueError('完整分数的类别无效或分页期间变化')
        classes = names
        columns = [classes.index(c) for c in SPOKEN_CLASSES if c in classes]
        a, b, rows = page.get('frame_onsets_s'), page.get('frame_offsets_s'), page.get('scores')
        if not all(isinstance(v, list) for v in (a, b, rows)) or not len(a) == len(b) == len(rows):
            raise ValueError('完整分数的帧数组不一致')
        for start, end, row in zip(a, b, rows):
            if (not number(start) or not number(end) or start >= end or start >= duration or end <= 0
                    or (starts and (start <= starts[-1] or end <= ends[-1] or start > ends[-1] + 1e-5))
                    or not isinstance(row, list) or len(row) != len(classes)
                    or not all(number(v) and 0 <= v <= 1 for v in row)):
                raise ValueError('完整分数存在无效时间、缺帧或概率')
            starts.append(start)
            ends.append(end)
            scores.append([row[c] for c in columns])
        next_start = page.get('next_start_s')
        if next_start is None:
            break
        if not a or not number(next_start) or not cursor < next_start < duration or abs(next_start - b[-1]) > 1e-5:
            raise ValueError('完整分数分页游标无效')
        cursor = next_start
    missing = [c for c in SPOKEN_CLASSES if c not in classes]
    voice['class_coverage'] = {'available': [c for c in SPOKEN_CLASSES if c in classes], 'missing': missing}
    if missing:
        voice['reason'] = 'missing_class_scores' if len(missing) == len(SPOKEN_CLASSES) else 'partial_class_scores'
    elif not scores:
        voice['reason'] = 'missing_frame_scores'
    else:
        # Median each class first, then union. Symmetric boundary padding repeats
        # the edge frame, exactly as the existing gun policy's three-frame median.
        active = []
        contributing = []
        for i, row in enumerate(scores):
            check_cancelled()
            found = [SPOKEN_CLASSES[c] for c in range(len(row))
                     if sorted((scores[max(0, i-1)][c], row[c], scores[min(len(scores)-1, i+1)][c]))[1] > THRESHOLD]
            active.append(bool(found))
            contributing.append(found)
        voice['status'] = 'completed'
        first = None
        for i in range(len(active) + 1):
            if i < len(active) and active[i] and first is None:
                first = i
            if first is not None and (i == len(active) or not active[i]):
                start, end = max(0., starts[first]), min(duration, ends[i-1])
                event_id = f'{identifier}:voice:v1:{len(voice["events"])+1}'
                labels = [c for c in SPOKEN_CLASSES if any(c in contributing[j] for j in range(first, i))]
                voice['events'].append({'event_id': event_id, 'category': 'voice', 'start_s': start,
                                        'end_s': end, 'contributing_classes': labels})
                voice['candidates'].append({'candidate_id': event_id + ':range', 'category': 'voice',
                                            'event_ids': [event_id], 'start_s': max(0., start-1.),
                                            'end_s': min(duration, end+1.)})
                first = None
    return {**result, 'categories': categories}
