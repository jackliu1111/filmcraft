/* THROWAWAY PRODUCT PROTOTYPE — deterministic fixtures, no model calls or storage. */
(function () {
  'use strict';

  var MIN_DURATION = 0.25;

  function clone(value) { return JSON.parse(JSON.stringify(value)); }
  function nextId(state, prefix) { return prefix + '-' + state.nextId++; }
  function clamp(value, low, high) { return Math.min(high, Math.max(low, value)); }
  function activeSequence(state) {
    return state.sequences.find(function (sequence) { return sequence.id === state.activeSequenceId; });
  }
  function sequenceDuration(sequence) {
    return sequence.clips.reduce(function (end, clip) { return Math.max(end, clip.start + clip.duration); }, 0);
  }
  function eventById(state, id) { return state.events.find(function (event) { return event.id === id; }); }

  function makeTrack(state, sequence) {
    if (sequence.tracks.length) return sequence.tracks[0];
    var track = { id: nextId(state, 'track'), number: 1, name: '声画剪辑' };
    sequence.tracks.push(track);
    return track;
  }

  // A sequence is one continuous edit. Video and its linked audio share each clip.
  function closeGaps(sequence) {
    var cursor = 0;
    sequence.clips.forEach(function (clip) {
      clip.start = cursor;
      cursor += clip.duration;
    });
  }

  function resolveInsertTime(sequence, at) {
    if (!sequence) return null;
    var duration = sequenceDuration(sequence);
    if (at === undefined) return duration;
    at = Number(at);
    if (!Number.isFinite(at)) return null;
    at = clamp(at, 0, duration);
    var nearest = 0;
    sequence.clips.forEach(function (clip) {
      var boundary = clip.start + clip.duration;
      // Equidistant drops insert after the clip, never split its source range.
      if (Math.abs(boundary - at) <= Math.abs(nearest - at)) nearest = boundary;
    });
    return nearest;
  }

  function clipAtTime(state, time, trackId) {
    var sequence = activeSequence(state);
    if (!sequence) return null;
    return sequence.clips.find(function (clip) {
      return time >= clip.start && time < clip.start + clip.duration &&
        (trackId == null || clip.trackId === trackId);
    }) || null;
  }

  function normalizeRange(state, start, end) {
    start = clamp(Number(start), 0, state.source.duration);
    end = clamp(Number(end), 0, state.source.duration);
    if (!Number.isFinite(start) || !Number.isFinite(end)) return null;
    if (start > end) { var swap = start; start = end; end = swap; }
    if (end - start < MIN_DURATION) {
      end = Math.min(state.source.duration, start + MIN_DURATION);
      start = Math.max(0, end - MIN_DURATION);
    }
    return { start: start, end: end };
  }

  function freezeBaseline(sequence) {
    if (!sequence.readOnly) return sequence;
    sequence.tracks.forEach(Object.freeze);
    Object.freeze(sequence.tracks);
    sequence.clips.forEach(function (clip) {
      if (clip.eventIds) Object.freeze(clip.eventIds);
      Object.freeze(clip);
    });
    Object.freeze(sequence.clips);
    if (sequence.inputRanges) {
      sequence.inputRanges.forEach(Object.freeze);
      Object.freeze(sequence.inputRanges);
    }
    return Object.freeze(sequence);
  }

  function checkpoint(state, label) {
    state.history.push({
      label: label,
      sequences: clone(state.sequences),
      activeSequenceId: state.activeSequenceId
    });
  }

  function createState() {
    var state = {
      prototype: true,
      fixtureNotice: '120 秒交互演示：画面来自本机真实录像的静音代理，枪声、人声标记均为预设，不代表真实 AI 识别效果。',
      nextId: 1,
      source: {
        id: 'demo-source',
        name: 'Apex_排位练习_演示.mp4',
        duration: 120,
        width: 3840,
        height: 2160,
        frameRate: 60,
        audioTracks: [{ id: 'mixed', name: '游戏声与语音混合', channels: 2 }],
        fixture: true
      },
      selectedId: null,
      events: [],
      ranges: {},
      sequences: [],
      activeSequenceId: null,
      history: []
    };
    var fixtures = [
      ['gun', 10, 12, 0.96, '枪声 01', '教学示例：识别 10–12 秒，默认待剪 5–17 秒，可调整到 8–16 秒。'],
      ['gun', 27.5, 30, 0.94, '枪声 02', '与人声 02 重叠，用于演示跨类别重复使用和自动合并。'],
      ['gun', 46, 48.5, 0.91, '枪声 03', '紧接人声 03，加入两类后可观察组稿合并。'],
      ['gun', 61, 64, 0.98, '枪声 04', '独立枪声组，用于拖入时间线中间并验证插入位移。'],
      ['gun', 81, 84.5, 0.93, '枪声 05', '人声覆盖的交战，用于检查部分重叠提示。'],
      ['gun', 99, 101, 0.89, '枪声 06', '较短枪声组，用于比较默认留白和手动剪短。'],
      ['gun', 113.5, 116, 0.95, '枪声 07', '靠近片尾，默认结束范围被截断到原片 120 秒。'],
      ['voice', 3.5, 6.5, 0.92, '人声 01', '片头人声；仅标记出现人声，不区分说话人，不转写。'],
      ['voice', 23, 31.5, 0.96, '人声 02', '与枪声 02 重叠，用于比较手动重复和自动去重。'],
      ['voice', 49, 53, 0.88, '人声 03', '交战后的语音，默认前后各留 1 秒。'],
      ['voice', 77, 86, 0.95, '人声 04', '覆盖枪声 05 的较长人声段。'],
      ['voice', 106, 110, 0.90, '人声 05', '片尾前的人声段，与临近枪声的留白存在交叠。']
    ];
    fixtures.forEach(function (fixture) {
      var event = {
        id: nextId(state, fixture[0]), kind: fixture[0], start: fixture[1], end: fixture[2],
        confidence: fixture[3], label: fixture[4], description: fixture[5], fixture: true
      };
      var padding = event.kind === 'gun' ? 5 : 1;
      state.events.push(event);
      state.ranges[event.id] = {
        start: Math.max(0, event.start - padding),
        end: Math.min(state.source.duration, event.end + padding),
        edited: false
      };
    });
    var manual = { id: nextId(state, 'sequence'), name: '手动选片', tracks: [], clips: [], readOnly: false, baselineId: null };
    state.sequences.push(manual);
    state.activeSequenceId = manual.id;
    state.selectedId = state.events[0].id;
    return state;
  }

  function selectEvent(state, id) {
    var event = eventById(state, id);
    if (event) state.selectedId = id;
    return event || null;
  }

  function setRange(state, id, start, end) {
    var event = eventById(state, id);
    var range = normalizeRange(state, start, end);
    if (!event || !range) return null;
    var padding = event.kind === 'gun' ? 5 : event.kind === 'voice' ? 1 : 0;
    range.edited = Math.abs(range.start - Math.max(0, event.start - padding)) > 0.001 ||
      Math.abs(range.end - Math.min(state.source.duration, event.end + padding)) > 0.001;
    state.ranges[id] = range;
    return range;
  }

  function addManualRange(state, start, end) {
    var range = normalizeRange(state, start, end);
    if (!range) return null;
    var count = state.events.filter(function (event) { return event.kind === 'manual'; }).length + 1;
    var event = {
      id: nextId(state, 'manual'), kind: 'manual', start: range.start, end: range.end,
      confidence: null, label: '手动补片 ' + String(count).padStart(2, '0'),
      description: '用户在原片上划选的范围，不是模型识别事件。', fixture: false
    };
    state.events.push(event);
    state.ranges[event.id] = { start: range.start, end: range.end, edited: false };
    state.selectedId = event.id;
    return event;
  }

  function makeClip(state, event, sourceStart, sourceEnd, at, eventIds, trackId) {
    return {
      id: nextId(state, 'clip'), eventId: event ? event.id : null,
      eventIds: eventIds || (event ? [event.id] : []),
      kind: event ? event.kind : 'mixed', label: event ? event.label : '组稿片段',
      sourceId: state.source.id, sourceStart: sourceStart, sourceEnd: sourceEnd,
      start: at, duration: sourceEnd - sourceStart, linked: true, trackId: trackId
    };
  }

  function insertClip(state, eventId, at) {
    var sequence = activeSequence(state);
    var event = eventById(state, eventId);
    var range = state.ranges[eventId];
    if (!sequence || sequence.readOnly || !event || !range) return null;
    at = resolveInsertTime(sequence, at);
    if (at === null) return null;
    var createdTrack = !sequence.tracks.length;
    checkpoint(state, '插入 ' + event.label);
    var track = makeTrack(state, sequence);
    var inserted = makeClip(state, event, range.start, range.end, at, null, track.id);
    var index = sequence.clips.findIndex(function (clip) { return clip.start >= at; });
    if (index < 0) index = sequence.clips.length;
    sequence.clips.splice(index, 0, inserted);
    closeGaps(sequence);
    return { clip: inserted, sequence: sequence, track: track, createdTrack: createdTrack };
  }

  function trimClip(state, clipId, start, end) {
    var sequence = activeSequence(state);
    var clip = sequence && sequence.clips.find(function (item) { return item.id === clipId; });
    var range = normalizeRange(state, start, end);
    if (!clip || sequence.readOnly || !range) return null;
    checkpoint(state, '调整 ' + clip.label);
    clip.sourceStart = range.start;
    clip.sourceEnd = range.end;
    clip.duration = range.end - range.start;
    closeGaps(sequence);
    return clip;
  }

  function removeClip(state, id) {
    var sequence = activeSequence(state);
    var clip = sequence && sequence.clips.find(function (item) { return item.id === id; });
    if (!clip || sequence.readOnly) return null;
    checkpoint(state, '移除 ' + clip.label);
    sequence.clips = sequence.clips.filter(function (item) { return item.id !== id; });
    closeGaps(sequence);
    return clip;
  }

  function undo(state) {
    var previous = state.history.pop();
    if (!previous) return null;
    state.sequences = previous.sequences.map(freezeBaseline);
    state.activeSequenceId = previous.activeSequenceId;
    return previous.label;
  }

  function createDraft(state, includeVoice) {
    var inputRanges = state.events.filter(function (event) {
      return event.kind === 'gun' || (includeVoice && event.kind === 'voice');
    }).map(function (event) {
      var range = state.ranges[event.id];
      return { start: range.start, end: range.end, eventId: event.id, kind: event.kind, edited: range.edited };
    }).sort(function (a, b) { return a.start - b.start; });
    if (!inputRanges.length) return null;
    checkpoint(state, '按当前范围生成草稿');
    var merged = [];
    inputRanges.forEach(function (range) {
      var previous = merged[merged.length - 1];
      if (previous && range.start <= previous.end) {
        previous.end = Math.max(previous.end, range.end);
        previous.eventIds.push(range.eventId);
      } else {
        merged.push({ start: range.start, end: range.end, eventIds: [range.eventId] });
      }
    });
    var draftNumber = state.sequences.filter(function (sequence) { return sequence.readOnly; }).length + 1;
    var baseline = {
      id: nextId(state, 'sequence'), name: '组稿 ' + draftNumber + ' · 原稿',
      tracks: [], clips: [], readOnly: true, baselineId: null,
      inputRanges: clone(inputRanges), includeVoice: Boolean(includeVoice)
    };
    var baselineTrack = makeTrack(state, baseline);
    var cursor = 0;
    merged.forEach(function (range, index) {
      var clip = makeClip(state, null, range.start, range.end, cursor, range.eventIds.slice(), baselineTrack.id);
      clip.label = '组稿片段 ' + String(index + 1).padStart(2, '0');
      clip.kind = includeVoice ? 'mixed' : 'gun';
      baseline.clips.push(clip);
      cursor += clip.duration;
    });
    var copy = {
      id: nextId(state, 'sequence'), name: '组稿 ' + draftNumber + ' · 编辑副本',
      tracks: [], clips: [], readOnly: false, baselineId: baseline.id
    };
    var copyTrack = makeTrack(state, copy);
    copy.clips = baseline.clips.map(function (clip) {
      var editable = clone(clip);
      editable.id = nextId(state, 'clip');
      editable.trackId = copyTrack.id;
      return editable;
    });
    state.sequences.push(freezeBaseline(baseline), copy);
    state.activeSequenceId = copy.id;
    return { baseline: baseline, copy: copy, mergedRanges: clone(merged) };
  }

  function switchSequence(state, id) {
    var sequence = state.sequences.find(function (item) { return item.id === id; });
    if (sequence) state.activeSequenceId = id;
    return sequence || null;
  }

  function overlapInfo(state, eventId) {
    var range = state.ranges[eventId];
    var sequence = activeSequence(state);
    var intervals = range && sequence ? sequence.clips.filter(function (clip) {
      return clip.sourceId === state.source.id && clip.sourceStart < range.end && clip.sourceEnd > range.start;
    }).map(function (clip) {
      return { start: Math.max(range.start, clip.sourceStart), end: Math.min(range.end, clip.sourceEnd) };
    }).sort(function (a, b) { return a.start - b.start; }) : [];
    var merged = [];
    intervals.forEach(function (interval) {
      var previous = merged[merged.length - 1];
      if (previous && interval.start <= previous.end) previous.end = Math.max(previous.end, interval.end);
      else merged.push({ start: interval.start, end: interval.end });
    });
    var used = merged.reduce(function (sum, interval) { return sum + interval.end - interval.start; }, 0);
    var coverage = range ? used / (range.end - range.start) : 0;
    var status = coverage >= 0.99999 ? 'used' : coverage > 0 ? 'partial' : 'unused';
    return {
      status: status, label: status === 'used' ? '已使用' : status === 'partial' ? '部分重叠' : '未使用',
      overlapSeconds: used, usedSeconds: used, coverage: coverage, matchingClips: intervals.length
    };
  }

  function snapshot(state) {
    var saved = clone(state);
    saved.history = [];
    return saved;
  }

  function restoreSnapshot(saved) {
    var state = typeof saved === 'string' ? JSON.parse(saved) : clone(saved);
    state.history = [];
    state.sequences = state.sequences.map(freezeBaseline);
    return state;
  }

  function formatTime(seconds) {
    seconds = Math.max(0, Number(seconds) || 0);
    var tenths = Math.round(seconds * 10);
    var minutes = Math.floor(tenths / 600);
    var rest = (tenths % 600) / 10;
    return String(minutes).padStart(2, '0') + ':' + rest.toFixed(1).padStart(4, '0');
  }

  window.PrototypeState = {
    createState: createState, selectEvent: selectEvent, setRange: setRange,
    addManualRange: addManualRange, insertClip: insertClip, trimClip: trimClip,
    removeClip: removeClip, undo: undo, createDraft: createDraft,
    switchSequence: switchSequence, snapshot: snapshot, restoreSnapshot: restoreSnapshot,
    overlapInfo: overlapInfo, activeSequence: activeSequence, clipAtTime: clipAtTime,
    sequenceDuration: sequenceDuration, resolveInsertTime: resolveInsertTime, formatTime: formatTime,
    MIN_DURATION: MIN_DURATION
  };
}());
