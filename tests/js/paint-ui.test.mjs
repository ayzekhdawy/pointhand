import {test} from 'node:test';
import assert from 'node:assert/strict';
import {paintControl, verifyPaintSetup, validateSelectors} from '../../scripts/lib/paint-ui.mjs';

test('Turkish and English Paint controls resolve by role with fresh indices', () => {
  for (const [edit,hex,ok] of [['Renkleri düzenle','RGB onaltılık','Tamam'],['Edit colors','Hex','OK']]) {
    const tree=`[7] Button "${edit}"\n[22] Edit "${hex}" value="#123456" editable\n[31] Button "${ok}"`;
    assert.equal(paintControl(tree,'editColors').index,7);
    assert.equal(paintControl(tree,'hex').value,'#123456');
    assert.equal(paintControl(tree,'confirm').index,31);
  }
});
test('ambiguous or missing controls stop instead of guessing', () => {
  assert.throws(()=>paintControl('[1] Button "OK"\n[2] Button "Tamam"','confirm'),/found 2/);
  assert.throws(()=>paintControl('[1] Edit "OK"','confirm'),/found 0/);
  assert.equal(paintControl('[1] Button "OK" offscreen\n[2] Button "OK" disabled\n[3] Button "OK"','confirm').index,3);
});
test('explicit selectors support other observed UI languages and reject malformed mappings', () => {
  assert.equal(paintControl('[8] Button "Bestätigen"','confirm',{confirm:['Bestätigen']}).index,8);
  assert.throws(()=>validateSelectors({confirm:[]}),/Invalid Paint selector/);
  assert.throws(()=>validateSelectors({unexpected:['OK']}),/Invalid Paint selector/);
});
test('setup checks preserve dimensions and pen size across Turkish and English units', () => {
  for (const unit of ['piksel','pixels','px']) verifyPaintSetup(`[0] Text "430 × 640px"\n[1] Slider "Size" value="2 ${unit}"`,{canvas:[430,640],pen_size:2});
  assert.throws(()=>verifyPaintSetup('[0] Text "640 × 430px"\n[1] Slider value="2 pixels"',{canvas:[430,640],pen_size:2}),/dimensions/);
  assert.throws(()=>verifyPaintSetup('[0] Text "430 × 640px"\n[1] Edit value="2 pixels"',{canvas:[430,640],pen_size:2}),/pencil size/);
});
