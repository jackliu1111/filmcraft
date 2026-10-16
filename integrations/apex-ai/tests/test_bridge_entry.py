"""Verify the relocated entry against independently configured backend checkouts."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
ENTRY = ROOT / 'integrations/apex-ai/bridge.py'


class BridgeEntryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.folder = Path(self.temp.name)
        self.backend = self.folder / 'backend 有空格'
        (self.backend / 'integrations/filmcraft').mkdir(parents=True)
        self.cwd = self.folder / 'unrelated'
        self.cwd.mkdir()

    def tearDown(self):
        self.temp.cleanup()

    def run_entry(self, backend=None):
        env = dict(os.environ)
        env.pop('APEX_AI_DATA_DIR', None)
        env['APEX_SERVICE_ROOT'] = str(backend or self.backend)
        return subprocess.run([sys.executable, str(ENTRY), 'probe', '--input', '/样本/片段.mkv'],
                              cwd=self.cwd, env=env, text=True, capture_output=True, timeout=10)

    def write_backend(self, text):
        (self.backend / 'integrations/filmcraft/bridge.py').write_text(text)

    def test_explicit_backend_from_unrelated_cwd_preserves_protocol(self):
        (self.backend / 'backend_helper.py').write_text("IDENTITY = 'configured-backend'\n")
        self.write_backend(
            "import json\nfrom backend_helper import IDENTITY\nprint('backend import log')\n"
            "def main(argv, *, env):\n"
            " print(json.dumps({'type':'media','backend':IDENTITY,'args':argv,'data_dir':env['APEX_AI_DATA_DIR']}))\n"
            " return 0\n")
        result = self.run_entry()
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual(1, len(result.stdout.splitlines()))
        record = json.loads(result.stdout)
        self.assertEqual('configured-backend', record['backend'])
        self.assertEqual(['probe', '--input', '/样本/片段.mkv'], record['args'])
        self.assertEqual(str(ROOT / 'outputs/apex-ai/model-data'), record['data_dir'])
        self.assertIn('backend import log', result.stderr)

    def test_explicit_missing_backend_does_not_fall_back(self):
        result = self.run_entry(self.folder / 'missing')
        self.assertNotEqual(0, result.returncode)
        self.assertEqual('backend_unavailable', json.loads(result.stdout)['code'])
        self.assertNotIn('Traceback', result.stderr)

    def test_missing_backend_dependency_is_structured(self):
        self.write_backend('import definitely_missing_filmcraft_test_dependency\n')
        result = self.run_entry()
        self.assertNotEqual(0, result.returncode)
        self.assertEqual('backend_import_failed', json.loads(result.stdout)['code'])
        self.assertNotIn('Traceback', result.stderr)

    def test_backend_failure_is_structured(self):
        self.write_backend("def main(argv, *, env):\n raise RuntimeError('controlled failure')\n")
        result = self.run_entry()
        self.assertNotEqual(0, result.returncode)
        self.assertEqual('backend_failed', json.loads(result.stdout)['code'])
        self.assertIn('controlled failure', json.loads(result.stdout)['message'])


class CategorizedAnalysisTests(BridgeEntryTests):
    """Exercise NDJSON, with the shared service's documented score operations."""
    def analysis(self, classes=None, frames=None, onsets=None, offsets=None, error=None):
        names = ['Speech', 'Male speech, man speaking', 'Female speech, woman speaking',
                 'Child speech, kid speaking', 'Conversation', 'Narration, monologue',
                 'Whispering', 'Speech synthesizer', 'Shout', 'Children shouting']
        classes = names if classes is None else classes
        # Speech alone is low; two speech subclasses sustain the middle event.
        frames = frames or [[0.0] * len(classes), [.0, .9] + [0.] * (len(classes)-2),
                            [.0, .8] + [0.] * (len(classes)-2), [0.0] * len(classes)]
        fixture = {'classes':classes, 'scores':frames,
                   'frame_onsets_s': onsets or [0., 1., 2., 3.],
                   'frame_offsets_s': offsets or [1., 2., 3., 4.]}
        (self.backend / 'fixture.json').write_text(json.dumps(fixture))
        self.write_backend("""import json
from pathlib import Path
from types import SimpleNamespace
ROOT = Path(__file__).resolve().parents[2]
def record():
 return {'schema_version':1, 'time_unit':'seconds', 'result_id':'a'*32,
         'prediction_id':'a'*32, 'media':{'duration_s':4., 'source':{'path':'source'}},
         'prediction':{'configuration':{}}, 'events':[{'event_id':'g1','start_s':1.,'end_s':3.}],
         'candidates':[{'candidate_id':'c1','start_s':0.,'end_s':4.}], 'selection':[]}
def main(argv, *, env, output):
 output.write(json.dumps({'type':'progress','stage':'cache_rebuild'})+'\\n')
 output.write(json.dumps({'type':'result','cached':True,'result':record()})+'\\n')
 (ROOT / 'closed').write_text('yes')
 return 0
def configuration(env, *, analyze):
 class Config:
  def service_arguments(self): return {}
 return Config()
class ParentGuard:
 def __init__(self,pid): pass
 def __enter__(self): return self
 def __exit__(self,*args): pass
 def check(self): pass
class ModelService:
 def __init__(self,**kwargs):
  assert (ROOT/'closed').exists(), 'original service must close first'
  self.pages=0
 def validate_result(self,result):
  (ROOT/'validated').write_text('yes')
 def execute(self,op,request):
  assert (ROOT/'validated').exists()
  if op=='get_result': return record()
  assert op=='read_scores', 'must not invoke inference'
  f=json.loads((ROOT/'fixture.json').read_text())
  start=0 if self.pages==0 else 2
  self.pages+=1
  f['prediction_id']='a'*32
  f['scores']=f['scores'][start:start+2]
  f['frame_onsets_s']=f['frame_onsets_s'][start:start+2]
  f['frame_offsets_s']=f['frame_offsets_s'][start:start+2]
  f['next_start_s']=f['frame_offsets_s'][-1] if self.pages==1 else None
  return f
 def close(self,**kwargs):
  (ROOT/'pages').write_text(str(self.pages))
""")
        env = dict(os.environ, APEX_SERVICE_ROOT=str(self.backend))
        proc = subprocess.run([sys.executable, str(ENTRY), 'analyze', '--input', '/source', '--audio-stream','1'],
                              cwd=self.cwd, env=env, capture_output=True, text=True, timeout=10)
        records = [json.loads(line) for line in proc.stdout.splitlines()]
        if error is not None:
            self.assertNotEqual(0, proc.returncode)
            self.assertEqual('error', records[-1]['type'])
            self.assertIn(error, records[-1]['message'])
            self.assertFalse(any(e['type']=='result' for e in records))
            return records[-1]
        self.assertEqual(0, proc.returncode, proc.stdout + proc.stderr)
        return records[-1]['result']

    def test_cached_scores_add_voice_without_new_inference_and_preserve_gun_schema(self):
        result = self.analysis()
        self.assertEqual([{'event_id':'g1','start_s':1.,'end_s':3.}], result['events'])
        self.assertEqual(result['events'], result['categories']['gunshot']['events'])
        voice = result['categories']['voice']
        self.assertEqual('completed', voice['status'])
        self.assertEqual('filmcraft-spoken-voice-v1', voice['postprocess']['version'])
        self.assertEqual((1.,3.), (voice['events'][0]['start_s'],voice['events'][0]['end_s']))
        self.assertEqual((0.,4.), (voice['candidates'][0]['start_s'],voice['candidates'][0]['end_s']))
        self.assertEqual('2', (self.backend/'pages').read_text())


    def test_missing_voice_columns_differ_from_successful_zero_candidates(self):
        missing = self.analysis(classes=['Gunshot, gunfire', 'Machine gun'])['categories']['voice']
        self.assertEqual('unavailable', missing['status'])
        self.assertEqual('missing_class_scores', missing['reason'])
        zero = self.analysis(frames=[[.2]*10 for _ in range(4)])['categories']['voice']
        self.assertEqual('completed', zero['status'])
        self.assertEqual([], zero['events'])
        self.assertEqual([], zero['candidates'])

    def test_partial_class_support_is_reported_without_inventing_complete_voice_support(self):
        voice = self.analysis(classes=['Speech', 'Conversation'])['categories']['voice']
        self.assertEqual('unavailable', voice['status'])
        self.assertEqual('partial_class_scores', voice['reason'])
        self.assertEqual(['Speech','Conversation'], voice['class_coverage']['available'])
        self.assertEqual(8, len(voice['class_coverage']['missing']))

    def test_median_spans_score_pages_and_video_times_are_not_shifted_twice(self):
        # The persistent activation straddles the two-page boundary; the first
        # frame starts before video zero, and the final frame ends after it.
        frames = [[0.]*10, [0.,.8]+[0.]*8, [0.,.9]+[0.]*8, [0.]*10]
        voice = self.analysis(frames=frames, onsets=[-.021,.979,1.979,2.979],
                              offsets=[.979,1.979,2.979,3.979])['categories']['voice']
        self.assertEqual(1, len(voice['events']))
        self.assertEqual((.979,2.979), (voice['events'][0]['start_s'],voice['events'][0]['end_s']))
        self.assertEqual((0.,3.979), (voice['candidates'][0]['start_s'],voice['candidates'][0]['end_s']))


    def test_invalid_prediction_timing_cannot_publish_a_completed_result(self):
        self.analysis(onsets=[0.,1.,1.,3.], offsets=[1.,2.,3.,4.], error='无效时间')


if __name__ == '__main__':
    unittest.main()
