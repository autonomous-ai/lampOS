"""DISABLED current Mood fresh full Expression gate; prior failures/controls provide integrity only, never runtime reuse."""
import hashlib, importlib.util, json, os, re, shutil, stat, subprocess, sys
from pathlib import Path
sys.dont_write_bytecode=True
E=Path(__file__).resolve().parent
P=json.loads((E/'plan.json').read_text())
def sha(p):
 with Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def save(n,v):
 with (E/n).open('x') as f:json.dump(v,f,indent=2);f.write('\n');f.flush();os.fsync(f.fileno())
def bound(item):
 p=Path(item['path']);assert p.resolve().is_relative_to(E.parent) and sha(p)==item['sha256'] and p.stat().st_size==item['bytes'];return json.loads(p.read_text())
def snapshot(root,names):
 index=subprocess.check_output(['git','--no-optional-locks','-C',str(root),'ls-files','--stage','-z']);found={os.fsdecode(row.split(b'\t',1)[1]) for row in index.split(b'\0') if row};assert found==set(names)
 out={}
 for n in sorted(names):
  p=root/n;a=p.lstat();data=os.fsencode(os.readlink(p)) if p.is_symlink() else p.read_bytes();b=p.lstat();assert (a.st_ino,a.st_size,a.st_mode,a.st_mtime_ns)==(b.st_ino,b.st_size,b.st_mode,b.st_mtime_ns)
  out[n]={'sha256':hashlib.sha256(data).hexdigest(),'bytes':len(data),'mode':stat.S_IMODE(b.st_mode),'kind':'symlink' if p.is_symlink() else 'file','mtime_ns':b.st_mtime_ns,'device':b.st_dev,'inode':b.st_ino}
 return out
def semantic(rows):return {n:{k:r[k] for k in ['sha256','bytes','mode','kind']} for n,r in rows.items()}
def artifacts(raw):
 rows=[json.loads(s) for s in raw.splitlines()];assert sum(r.get('reason')=='build-finished' for r in rows)==1 and any(r.get('reason')=='build-finished' and r.get('success') for r in rows)
 return [r for r in rows if r.get('reason')=='compiler-artifact']
def main():
 assert P['execution_ready'] and P['canonical_alias_bound'] and P['compiler_lease']=='root-mood-3aa-full-expression'
 L=json.loads((E/'launch.json').read_text());assert L['controller_sha256']==sha(__file__) and L['plan_sha256']==sha(E/'plan.json') and L['compiler_lease']==P['compiler_lease'] and L['root_review_approved'] and L['parent_closed'] and L['exclusive_target'] and L['previous_owner_released'] and L['full_runtime_authorized'] and L['exclusive_source_copy'] and L['exclusive_owner'] and L['sealed_no_legitimate_writer'] and L['require_escalated'] and L['concurrency_context']['idle_host_claim'] is False
 assert P['runtime_reuse_authorized'] is False
 history=bound(P['historical_inputs']);assert history['runtime_reuse'] is False
 for record in history['records']:
  for item in record['receipts'].values():bound(item)
 H=bound(P['handoff_binding']);parent=bound(H['parent_terminal']);native=bound(H['parent_native']);before=bound(H['parent_source_before']);after=bound(H['parent_source_after']);protected=bound(H['protected_after']);retired=bound(H['actual_after']);release=bound(H['release_receipt']);verified=bound(H['release_verified_bindings'])
 assert H['status']=='ROOT_RELEASED_READER_MODULES_ONLY' and parent['status']=='PASS' and native['exit_code']==0 and native['terminal.json']['sha256']==H['parent_terminal']['sha256'] and before==after
 assert parent['owned_groups_closed'] and parent['lease_released'] and parent['unresolved_owned_pids']==[] and parent['source_unchanged'] and parent['official_owner_unchanged'] and parent['sealed_and_actual_guards']
 expected_parent=bound(P['expected_parent_source_binding']);assert parent['head']==before['head']==P['expected_parent_head'] and parent['base']==P['expected_parent_base'] and parent['runtime_attempts']==2 and parent['module_counts']==H['parent_counts']==release['parent_module_counts']
 assert parent['module_counts']=={'session_reader::tests::':{'passed':4,'failed':0,'ignored':0},'service_trace::tests::':{'passed':6,'failed':0,'ignored':0}} and parent['Reader_cases_executed_once'] and parent['service_trace_cases_executed_once'] and parent['failed_modules']==[] and parent['full_Voice_runtime_qualification'] is False and release['parent_full_Voice_qualification'] is False and before==expected_parent
 for item in H['parent_commands'].values():
  row=bound(item);assert row['exit_code']==0 and row['direct_wait_status']==0 and row['owned_group_absent_after_cleanup'] and not row['unresolved_owned_pids'] and not row['timed_out'] and not row['disk_reserve_breached']
 for item in H['parent_source_profile_bindings'].values():bound(item)
 assert len(protected)==P['inherited_frozen_count']==parent['protected_count']==release['protected_count'] and len(retired)==parent['actual_count']==release['actual_count'] and set(protected)==set(verified['sealed']) and set(retired)==set(verified['actual'])
 assert release['status']=='INTEGRITY_RELEASE_FROM_READER_MODULE_PASS' and release['previous']=='sentinel-b7-reader-service-trace-gates-prep' and release['parent_native_exit']==0 and all(release[k] is True for k in ['all_protected_full_hash_metadata_verified','all_actual_full_hash_metadata_verified_before_retirement','actual_bindings_retired','all_owned_groups_previously_closed','lsof_no_open_target_file','no_historical_test_failure_waived']) and release['lsof_exit']==1
 for item in [H['lsof_stdout'],H['lsof_stderr']]:assert item['bytes']==0 and sha(item['path'])==item['sha256'] and Path(item['path']).stat().st_size==0
 keys=['sha256','bytes','mode','mtime_ns','device','inode']
 for n,v in protected.items():assert all(v[k]==verified['sealed'][n][k] for k in keys)
 for n,v in retired.items():assert all(v['bound'][k]==v['observed'][k]==verified['actual'][n][k] for k in keys)
 W,T,O=Path(P['worktree']),Path(P['target']),Path(P['permanent_worktree']);assert W.is_dir() and not W.is_symlink() and W!=O and T.is_dir() and not T.is_symlink() and [T.stat().st_dev,T.stat().st_ino]==P['target_dev_ino']==L['target_dev_ino']==release['target_dev_ino'] and release['target']==str(T)
 assert sha(E/'support.py')==P['support_sha256'];spec=importlib.util.spec_from_file_location('mood_current_supervisor',E/'support.py');S=importlib.util.module_from_spec(spec);spec.loader.exec_module(S)
 canonical_expected=bound(P['canonical_source_binding']);owner_expected=bound(P['official_source_binding']);assert canonical_expected['head']==owner_expected['head']==P['head'] and canonical_expected['status']==owner_expected['status']=='' and canonical_expected['branch']==P['canonical_branch'] and owner_expected['branch']==P['branch'] and {k:v for k,v in canonical_expected.items() if k!='branch'}=={k:v for k,v in owner_expected.items() if k!='branch'}
 canonical=canonical_expected['files'];new_frozen={};current_actual={};initial=None;owner_initial=None;guard_events=[]
 released_prior_actuals=False;test_driver_paths=set();alias_paths=set();alias_artifacts=[];alias_inodes={};alias_events=[]
 terminal={'status':'FAIL','head':P['head'],'base':P['base'],'branch':P['branch'],'authored_paths':P['candidate_paths'],'uncommitted_paths':[],'runtime_attempts':0,'runtime_reuse':False,'current_test_execution_claim':False,'scope':P['scope'],'controlled_overlay':False,'native_claim':False}
 def owner():
  for root,expected in [(W,canonical_expected),(O,owner_expected)]:
   def g(*a):return subprocess.check_output(['git','--no-optional-locks','-C',str(root),*a])
   assert g('rev-parse','HEAD').decode().strip()==P['head'] and g('rev-parse','HEAD^').decode().strip()==P['base'] and g('branch','--show-current').decode().strip()==expected['branch'] and g('status','--porcelain=v1','--untracked-files=all').decode()=='' and hashlib.sha256(g('ls-files','--stage','-z')).hexdigest()==expected['index_sha256']
 def stat_snapshot(root,names):
  index=subprocess.check_output(['git','--no-optional-locks','-C',str(root),'ls-files','--stage','-z']);found={os.fsdecode(row.split(b'\t',1)[1]) for row in index.split(b'\0') if row};assert found==set(names)
  result={}
  for n in sorted(names):
   st=(root/n).lstat();result[n]={'bytes':st.st_size,'mode':stat.S_IMODE(st.st_mode),'kind':'symlink' if stat.S_ISLNK(st.st_mode) else 'file','mtime_ns':st.st_mtime_ns,'device':st.st_dev,'inode':st.st_ino}
  return result
 def expected_stats(rows):return {n:{k:r[k] for k in ['bytes','mode','kind','mtime_ns','device','inode']} for n,r in rows.items()}
 def witnesses(hash_content=True):
  for n,b in {**protected,**new_frozen}.items():
   q=Path(n);st=q.stat();assert q.is_file() and not q.is_symlink() and q.resolve().is_relative_to(E.parent) and not any(q.resolve().is_relative_to(root) for root in [T,W,O])
   now={'bytes':st.st_size,'mode':st.st_mode&0o7777,'mtime_ns':st.st_mtime_ns,'device':st.st_dev,'inode':st.st_ino}
   if hash_content:now['sha256']=sha(q)
   assert all(now[k]==b[k] for k in now);after=q.stat();assert (st.st_dev,st.st_ino,st.st_size,st.st_mode,st.st_mtime_ns)==(after.st_dev,after.st_ino,after.st_size,after.st_mode,after.st_mtime_ns)
 def actuals(allow_alias_republish=False):
  observed={}
  for n,b in current_actual.items():
   q=Path(n);st=q.stat();assert q.is_file() and not q.is_symlink() and q.resolve().is_relative_to(T)
   now={'sha256':sha(q),'bytes':st.st_size,'mode':st.st_mode&0o7777,'mtime_ns':st.st_mtime_ns,'device':st.st_dev,'inode':st.st_ino};assert all(now[k]==v for k,v in b.items() if k!='inode');expected_inode=alias_inodes.get(n,b['inode'])
   if now['inode']!=expected_inode:
    assert allow_alias_republish and n in alias_paths and n not in alias_inodes;alias_inodes[n]=now['inode'];alias_events.append({'path':n,'before_inode':b['inode'],'after_inode':now['inode'],'boundary':'successful sole full Cargo Expression command after no-run; all other attributes strict'})
   observed[n]=now
  return observed
 def target_identity():assert not T.is_symlink() and [T.stat().st_dev,T.stat().st_ino]==P['target_dev_ino']
 def full(point):
  owner();target_identity();assert snapshot(W,canonical)==initial and snapshot(O,canonical)==owner_initial;witnesses(True);observed=actuals();guard_events.append({'point':point,'source_and_owner_scope':'full file set, bytes SHA256, mode/kind/size/mtime/device/inode and owner Git metadata','sealed_scope':'all inherited and new witness SHA256 plus strict metadata','sealed_count':len(protected)+len(new_frozen),'actual_scope':'all currently bound actuals SHA256 plus six attributes','actual_count':len(observed)})
 def boundary(point,allow_alias_republish=False):
  owner();target_identity();assert stat_snapshot(W,canonical)==expected_stats(initial) and stat_snapshot(O,canonical)==expected_stats(owner_initial);witnesses(False);observed=actuals(allow_alias_republish)
  guard_events.append({'point':point,'source_and_owner_scope':'strict entire file set and kind/mode/size/mtime/device/inode stats plus owner HEAD/branch/index/clean; no content rehash claimed','sealed_scope':'strict bytes/mode/mtime/device/inode stats, no SHA rehash claimed','actual_scope':'strict SHA256 and all attributes for every currently bound actual: none before compile, then8 after no-run; only prospective four helper alias inode events at successful full Cargo boundary' if current_actual else 'no mutable-actual binding between declared release and full post-no-run8 binding; inherited frozen witnesses remain strict','actual_count':len(observed),'actual_observations':observed})
 env=os.environ.copy()
 for k in list(env):
  if k.startswith(('LAMP_','CARGO_PROFILE_','CARGO_TARGET_')) or k in ['CARGO_BUILD_TARGET','RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','RUSTC_WRAPPER','RUSTC_WORKSPACE_WRAPPER','RUSTUP_TOOLCHAIN','RUST_TEST_THREADS']:env.pop(k)
 env.update(CARGO_TARGET_DIR=str(T),CARGO_BUILD_JOBS='1',CARGO_INCREMENTAL='0',CARGO_PROFILE_DEV_DEBUG='0',CARGO_PROFILE_TEST_DEBUG='0',CARGO_NET_OFFLINE='true',CARGO_TERM_COLOR='never');env['PATH']='/Users/autonomous/.cargo/bin:'+env.get('PATH','')
 runtime_result=None
 inventory={}
 def parse_runtime(receipt,out,err):
  markers=re.findall(r'^\s*Running .*? \((.*?)\)$',err,re.M);blocks=re.split(r'^running \d+ tests?$',out,flags=re.M)[1:];assert set(markers)==set(inventory) and len(markers)==len(inventory)
  totals={'passed':0,'failed':0,'ignored':0};groups=[]
  for exe,block in zip(markers,blocks):
   raw_rows=re.findall(r'^test (.*?) \.\.\. (ok|FAILED|ignored)(?:,.*)?$',block,re.M);expected=inventory[exe];rows=[(n.removesuffix(' - should panic') if n not in expected['all'] and n.endswith(' - should panic') and n.removesuffix(' - should panic') in expected['all'] else n,s) for n,s in raw_rows];counts={k:sum(s==v for _,s in rows) for k,v in [('passed','ok'),('failed','FAILED'),('ignored','ignored')]};assert sorted(n for n,s in rows)==sorted(expected['all']) and sorted(n for n,s in rows if s=='ignored')==sorted(expected['ignored'])
   summary=re.findall(r'^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out',block,re.M);assert summary==[(str(counts['passed']),str(counts['failed']),str(counts['ignored']),'0','0')]
   for k in totals:totals[k]+=counts[k]
   groups.append({'executable':exe,'rows':rows,'raw_rows':raw_rows,'counts':counts})
  docs=blocks[len(markers):];assert len(docs)==len(re.findall(r'^\s*Doc-tests ',err,re.M));save('observed-census.json',{'command_exit':receipt['exit_code'],'counts':totals,'targets':groups,'doc_blocks':docs,'ordinary_filter_count':0});terminal.update(counts=totals,doc_blocks=docs,ordinary_filter_count=0,runtime_command_exit=receipt['exit_code'],runtime_command_completed=True,current_test_execution_claim=True)
  assert len(docs)==sum(t['doctest'] and t['kind']==['lib'] for t in package['targets']);doc_summaries=[re.findall(r'^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out',block,re.M) for block in docs];assert all(len(x)==1 for x in doc_summaries);save('derived-doc-census.json',{'summaries':doc_summaries,'full_blocks':docs,'no_historical_fixed_count_used':True});assert all(x[0][1]=='0' and x[0][3:]==('0','0') for x in doc_summaries)
  return totals,groups,docs
 def stage(label,args,cargo=True,allowed=(0,),phase=False,timeout=3600):
  nonlocal runtime_result
  if cargo:
   assert args[0] in ['fmt','metadata','clippy','build','test'] and (args[0]!='test' or '--no-run' in args or label=='tests' and args==['test','--offline','--locked','--jobs','1','-p','lamp-expression','--no-fail-fast','--','--test-threads=1'])
  else:assert label.startswith('inventory-') and args[0] in test_driver_paths and args[1:] in [['--list','--format','terse'],['--ignored','--list','--format','terse']]
  assert not S.OWNED_OPEN;boundary('before-'+label)
  command=['/Users/autonomous/.cargo/bin/cargo','+1.96.0',*args] if cargo else args
  if not cargo or label=='tests':save('consumed-executables-'+label+'.json',{'before_invocation':actuals(),'direct_consumed':args[0] if not cargo else 'all four actual test drivers and four helper bindings before full Cargo package runtime','direct_SHA256':sha(Path(args[0])) if not cargo else None})
  receipt=None
  try:receipt=S.run_command(command,W/'lampOS',env,E,'cmd-'+label,timeout)
  finally:
   if label=='tests':
    stdout_path=E/'cmd-tests.stdout.txt';stderr_path=E/'cmd-tests.stderr.txt';raw_out=stdout_path.read_text() if stdout_path.exists() else '';raw_err=stderr_path.read_text() if stderr_path.exists() else ''
    save('runtime-raw-status-before-postguards.json',{'command_receipt_available':receipt is not None,'command_exit':receipt.get('exit_code') if receipt else None,'raw_status_rows':re.findall(r'^test (.*?) \.\.\. (ok|FAILED|ignored)(?:,.*)?$',raw_out,re.M),'raw_summaries':re.findall(r'^test result:.*$',raw_out,re.M),'raw_stdout_sha256':sha(stdout_path) if stdout_path.exists() else None,'raw_stderr_sha256':sha(stderr_path) if stderr_path.exists() else None,'these_raw_rows_may_include_child_output_and_are_not_an_execution_census':True,'before_post_runtime_guards':True})
    try:
     if receipt is not None:runtime_result=parse_runtime(receipt,raw_out,raw_err)
     else:raise AssertionError('Full Cargo supervisor did not return a receipt; raw partial output retained')
    except BaseException as error:terminal['runtime_census_error']=repr(error);save('runtime-census-error-before-postguards.json',{'error':repr(error),'Rust_failure_or_unrun_never_waived':True})

  if label=='test-compile' and receipt['exit_code']==0:
   rows=[a for a in artifacts((E/receipt['stdout']).read_text()) if a.get('executable')];assert len(rows)==derived_executable_count
   for a in rows:
    q=Path(a['executable']);st=q.stat();assert q.is_file() and not q.is_symlink() and q.resolve().is_relative_to(T);current_actual[str(q)]={'sha256':sha(q),'bytes':st.st_size,'mode':st.st_mode&0o7777,'mtime_ns':st.st_mtime_ns,'device':st.st_dev,'inode':st.st_ino}
   save('post-no-run-current-actual-binding.json',current_actual)
  republish=label=='tests' and receipt['exit_code']==receipt['direct_wait_status']==0 and receipt['owned_group_absent_after_cleanup'] and not receipt['unresolved_owned_pids'] and not receipt['timed_out'] and not receipt['disk_reserve_breached'] and not receipt.get('unexpected_owned_descendants') and not re.search(r'^\s*(?:Compiling|Checking)\s',((E/receipt['stdout']).read_text()+'\n'+(E/receipt['stderr']).read_text()),re.M)
  try:boundary('after-'+label,republish)
  except BaseException as error:
   if label!='tests':raise
   terminal['runtime_boundary_guard_error']=repr(error);save('runtime-boundary-failure.json',{'error':repr(error),'command_exit':receipt['exit_code'],'guard_FAILURE_preserved':True,'next_action':'Read completed raw census only; no further invocation.'})
  if label=='tests':save('alias-republish-audit.json',{'eligible_boundary':republish,'derived_current_non_test_alias_artifacts':alias_artifacts,'current_derived_alias_paths':sorted(alias_paths),'inode_events':alias_events,'boundary_guard_passed':'runtime_boundary_guard_error' not in terminal,'strictness_requirement':'All other attributes, four test-driver inodes and every frozen inode always strict; any guard error retains FAIL.'})
  assert receipt['exit_code'] in allowed and receipt['direct_wait_status']==receipt['exit_code'] and receipt['owned_group_absent_after_cleanup'] and not receipt['unresolved_owned_pids'] and not receipt['timed_out'] and not receipt['disk_reserve_breached'] and not receipt.get('unexpected_owned_descendants')
  return receipt,(E/receipt['stdout']).read_text(),(E/receipt['stderr']).read_text()
 def forward_closure(metadata):
  ps={p['id']:p for p in metadata['packages']};ns={n['id']:n for n in metadata['resolve']['nodes']};root=next(i for i in ns if ps[i]['name']=='lamp-expression');ids={root}
  while True:
   larger=ids|{d['pkg'] for i in ids for d in ns[i]['deps'] if i==root or any(k['kind']!='dev' for k in d['dep_kinds'])}
   if larger==ids:return ids,ps
   ids=larger
 try:
  owner();initial=snapshot(W,canonical);assert semantic(initial)==semantic(canonical);owner_initial=snapshot(O,canonical);assert semantic(owner_initial)==semantic(canonical);assert expected_stats(initial)==bound(P['canonical_physical_binding']) and expected_stats(owner_initial)==bound(P['official_physical_binding']);full('initial');save('source-before.json',initial);save('owner-source-before.json',owner_initial);save('protected-before.json',protected);save('environment-hashes.json',{k:hashlib.sha256(v.encode()).hexdigest() for k,v in env.items()})
  save('historical-dispositions-retained.json',history)
  transition=bound(P['source_transition']);changed_paths=[n for n in sorted(set(before['files'])|set(canonical)) if before['files'].get(n)!=canonical.get(n)];assert changed_paths==transition['changed_paths']==P['parent_source_transition'] and transition['child_head']==P['head'] and transition['parent_snapshot']==H['parent_source_before'] and transition['child_count']==len(canonical)==P['source_count'];save('parent-source-transition-observed.json',transition)
  for root in [W,O]:assert not any(marker.encode() in (root/'lampOS/crates/expression/tests/runtime.rs').read_bytes() for marker in P['known_forbidden_diagnostic_markers'])
  save('current-source-diagnostic-marker-check.json',{'source':'lampOS/crates/expression/tests/runtime.rs','canonical_and_official':True,'known_markers':P['known_forbidden_diagnostic_markers'],'none_found':True})

  stage('fmt',['fmt','--all','--check'],timeout=120);terminal['fmt']=True
  _,raw,_=stage('metadata',['metadata','--offline','--locked','--format-version','1'],timeout=120);meta=json.loads(raw);assert Path(meta['workspace_root']).resolve()==W/'lampOS' and Path(meta['target_directory']).resolve()==T;save('metadata-response.json',meta)
  forward,forward_packages=forward_closure(meta);roots=[Path(forward_packages[i]['manifest_path']).parent.relative_to(W).as_posix()+'/' for i in forward if forward_packages[i]['source'] is None];assert any(forward_packages[i]['name']=='lamp-ipc' for i in forward)
  save('current-forward-input-bindings.json',{'current_head':P['head'],'source_count':len(canonical),'kind_policy':P['forward_closure_kind_policy'],'local_packages':sorted(forward_packages[i]['name'] for i in forward if forward_packages[i]['source'] is None),'new_changes_inside_forward_inputs':[n for n in P['parent_source_transition'] if any(n.startswith(root) for root in roots)],'resolved_package_source_features':[{k:forward_packages[i][k] for k in ['id','source','manifest_path','features']} for i in sorted(forward)],'runtime_reuse':False,'registry_scope':'Actual Cargo metadata source IDs/lock/config identity and emitted profile/features; no historical per-registry-file byte snapshot or transitive freshness claim'})
  packages={p['id']:p for p in meta['packages']};members=set(meta['workspace_members']);seed=[i for i in members if packages[i]['name']=='lamp-expression'];assert len(seed)==1;affected=set(seed)
  while True:
   wider=affected|{n['id'] for n in meta['resolve']['nodes'] if any(d['pkg'] in affected for d in n['deps'])}
   if wider==affected:break
   affected=wider
  package=packages[seed[0]];metadata_drivers=[t for t in package['targets'] if t['test'] and t['kind']!=['example']];metadata_helpers=[t for t in package['targets'] if t['kind'] in [['bin'],['example']]];derived_driver_count=len(metadata_drivers);derived_executable_count=derived_driver_count+len(metadata_helpers)
  assert derived_driver_count==P['expected_driver_count_reference'] and derived_executable_count==P['expected_executable_count_reference'];save('actual-package-target-census.json',{'package_id':package['id'],'driver_targets':metadata_drivers,'helper_targets':metadata_helpers,'drivers':derived_driver_count,'executable_outputs':derived_executable_count,'ordinary_test_count':'derive current inventories, never assumed340'})
  closure=sorted(packages[i]['name'] for i in affected&members);assert closure==P['expected_reverse_closure'];save('affected.json',{'seed':'lamp-expression','reverse_closure':closure});flags=[v for n in closure for v in ['-p',n]]
  def bindings(emitted,label):
   local=[];registry=[]
   for a in emitted:
    p=packages[a['package_id']]
    if p['source'] is not None:
     assert Path(a['manifest_path']).resolve()==Path(p['manifest_path']).resolve() and a['target'] in p['targets'] and isinstance(a['fresh'],bool) and set(a['features'])<=set(p['features']);registry.append({'artifact':a,'resolved_source':p['source'],'version':p['version']});continue
    assert Path(a['manifest_path']).resolve()==Path(p['manifest_path']).resolve() and Path(a['manifest_path']).resolve().is_relative_to(W) and Path(a['target']['src_path']).resolve().is_relative_to(W) and a['target'] in p['targets'] and isinstance(a['fresh'],bool)
    assert a['profile']['debuginfo']==0 and a['profile']['debug_assertions'] and a['profile']['overflow_checks'] and set(a['features'])<=set(p['features'])
    # All freshness flags are observed; the authored runtime driver is separately required fresh:false.
    prefix=Path(p['manifest_path']).resolve().parent.relative_to(W).as_posix()+'/';tracked={n:r for n,r in initial.items() if n.startswith(prefix)};assert tracked;local.append({'artifact':a,'tracked_package_source':tracked})
   assert local and any(a['package_id']==seed[0] for a in emitted);save('registry-artifact-bindings-'+label+'.json',{'artifacts':registry,'current_Cargo_lock':canonical['lampOS/Cargo.lock'],'current_tracked_Cargo_config':{n:r for n,r in canonical.items() if '/.cargo/' in n},'scope':'Current resolved registry source identity/manifest/target/full profile/features/fresh observation, not registry source-byte audit or transitive freshness claim'});return local
  boundary('declared-pre-build-release');save('retired-parent-actual-bindings.json',{'manifest':H['actual_after'],'count':len(retired),'Root_release_verified':True,'not_active_current_guards':True,'all_inherited_frozen_remain_strict':True});released_prior_actuals=True
  _,raw,_=stage('strict',['clippy','--offline','--locked','--all-targets','--jobs','1','--message-format=json',*flags,'--','-D','warnings'],phase=True);emitted=artifacts(raw);save('strict-all-artifacts.json',emitted);save('strict-local-bindings.json',bindings(emitted,'strict'));terminal.update(strict_packages=closure,Expression_artifact_freshness_observed_only=True,blanket_local_freshness_claim=False)
  _,raw,_=stage('workspace',['build','--offline','--locked','--workspace','--jobs','1','--message-format=json'],phase=True);emitted=artifacts(raw);bindings(emitted,'workspace');save('workspace-artifacts.json',emitted);terminal['workspace_build']=True
  common=['test','--offline','--locked','--jobs','1','-p','lamp-expression'];_,raw,_=stage('test-compile',[*common,'--no-run','--message-format=json'],phase=True);emitted=artifacts(raw);bindings(emitted,'test-compile');save('test-compiler-artifacts.json',emitted)
  package=packages[seed[0]];selected=[a for a in emitted if a.get('executable') and a['profile']['test']];assert selected and all(a['package_id']==package['id'] for a in selected)
  assert sorted((a['target']['name'],a['target']['kind']) for a in selected)==sorted((t['name'],t['kind']) for t in package['targets'] if t['test'] and t['kind']!=['example'])
  folder=E/'frozen';folder.mkdir()
  for n,a in enumerate(a for a in emitted if a.get('executable')):
   q=Path(a['executable']);st=q.stat();assert q.resolve().is_relative_to(T) and not q.is_symlink();b={'sha256':sha(q),'bytes':st.st_size,'mode':st.st_mode&0o7777,'mtime_ns':st.st_mtime_ns,'device':st.st_dev,'inode':st.st_ino};assert current_actual[str(q)]==b;assert shutil.disk_usage(E).free>P['operational_reserve_bytes']+st.st_size;f=folder/(str(n)+'-'+q.name);shutil.copy2(q,f);assert sha(f)==b['sha256'] and (f.stat().st_dev,f.stat().st_ino)!=(st.st_dev,st.st_ino);new_frozen[str(f)]={**b,'mode':f.stat().st_mode&0o7777,'mtime_ns':f.stat().st_mtime_ns,'device':f.stat().st_dev,'inode':f.stat().st_ino}
  save('actual-executables.json',current_actual);save('new-frozen.json',new_frozen)
  current_exes=[a for a in emitted if a.get('executable')]
  assert len(current_exes)==len(current_actual)==len(new_frozen)==derived_executable_count and len(selected)==derived_driver_count
  test_driver_paths={a['executable'] for a in selected};alias_artifacts=[a for a in current_exes if a['profile']['test'] is False and a['target']['kind'] in [['bin'],['example']]];alias_paths={a['executable'] for a in alias_artifacts}
  assert len(alias_paths)==4 and not alias_paths&test_driver_paths and all(a['package_id']==package['id'] for a in current_exes) and all(Path(a['executable'])==(T/'debug'/a['target']['name'] if a['target']['kind']==['bin'] else T/'debug'/'examples'/a['target']['name']) for a in alias_artifacts)
  save('prospective-alias-inode-policy.json',{'actual_derived_artifacts':alias_artifacts,**P['prospective_alias_inode_policy']})
  changed=[a for a in selected if a['target']['name']=='runtime' and Path(a['target']['src_path']).resolve()==W/'lampOS/crates/expression/tests/runtime.rs'];assert len(changed)==1 and changed[0]['fresh'] is False and P['require_changed_runtime_fresh_false']
  dep=Path(changed[0]['executable']).with_suffix('.d');assert dep.is_file();text=dep.read_text().replace('\\\n','');line=next(x for x in text.splitlines() if ': ' in x);import shlex;paths=shlex.split(line.split(': ',1)[1]);reached={}
  for name in paths:
   q=Path(name);q=q if q.is_absolute() else W/'lampOS'/q
   if q.resolve().is_relative_to(W):
    rel=q.resolve().relative_to(W).as_posix();assert rel in canonical and sha(q)==canonical[rel]['sha256'];reached[rel]=canonical[rel]
  assert 'lampOS/crates/expression/tests/runtime.rs' in reached;save('changed-runtime-source-profile-binding.json',{'artifact':changed[0],'depinfo_sha256':sha(dep),'actual_local_inputs':reached,'fresh_false_selected_only':True,'no_blanket_transitive_freshness_claim':True})
  forbidden=[x.encode() for x in P['known_forbidden_diagnostic_markers']]
  for a in current_exes:
   carry=b''
   with Path(a['executable']).open('rb') as f:
    while chunk:=f.read(1024*1024):
     data=carry+chunk;assert not any(x in data for x in forbidden),'Known private diagnostic marker in current executable';carry=data[-max(map(len,forbidden)):]
  save('known-diagnostic-marker-check.json',{'actual_count':len(current_exes),'known_markers':P['known_forbidden_diagnostic_markers'],'none_found':True,'unknown_marker_absence_claim':False})
  inventory={}
  for n,a in enumerate(selected):
   row={}
   for which,flags in [('all',[]),('ignored',['--ignored'])]:
    _,out,_=stage(f'inventory-{n}-{which}',[a['executable'],*flags,'--list','--format','terse'],False,timeout=P['inventory_timeout_seconds']);row[which]=[s.removesuffix(': test') for s in out.splitlines() if s.endswith(': test')];assert len(row[which])==len(set(row[which]))
   assert set(row['ignored'])<=set(row['all']);inventory[a['executable']]=row
  inventory_total=sum(len(row['all']) for row in inventory.values());inventory_ignored=sum(len(row['ignored']) for row in inventory.values());assert inventory_total>0;save('derived-inventory-counts.json',{'all':inventory_total,'ignored':inventory_ignored,'ordinary_expected':inventory_total-inventory_ignored,'no_historical_fixed_count_used':True})
  assert all(sum(row['all'].count(case) for row in inventory.values())==1 for case in P['required_cases']);save('inventory.json',inventory);terminal['runtime_attempts']=1
  receipt,out,err=stage('tests',[*common,'--no-fail-fast','--','--test-threads=1'],allowed=(0,101),phase=True,timeout=P['package_runtime_timeout_seconds'])
  assert 'runtime_census_error' not in terminal
  totals,groups,docs=runtime_result
  assert sum(totals.values())==inventory_total and totals['ignored']==inventory_ignored and receipt['exit_code']==0 and totals['failed']==0 and all(len(re.findall(r'^test result: ok\..*? 0 failed;',b,re.M))==1 for b in docs)
  assert all(sum(sum(n==case and s=='ok' for n,s in g['rows']) for g in groups)==1 for case in P['required_cases'])
  assert 'runtime_boundary_guard_error' not in terminal
  terminal.update(status='PASS',qualification_mode='fresh_full_package_runtime',runtime_reuse=False,current_test_execution_claim=True,affected_case_passed_once=True,required_cases_passed_once=P['required_cases'],qualified_committed_paths={n:canonical[n] for n in P['candidate_paths']},committed_head_matches_qualified_owner=P['head'])
 except BaseException as error:
  terminal['error']=repr(error)
 finally:
  terminal['unresolved_owned_pids']=sorted(S.OWNED_OPEN);terminal['lease_released']=not bool(S.OWNED_OPEN)
  try:
   owner();owner_after=snapshot(O,canonical);assert owner_after==owner_initial;save('owner-source-after.json',owner_after);terminal['permanent_owner_unchanged']=True
   full('final');save('actual-executables-after.json',{n:{**row,'inode':alias_inodes.get(n,row['inode'])} for n,row in current_actual.items()});save('source-after.json',snapshot(W,canonical));save('protected-after.json',{**protected,**new_frozen});terminal.update(source_unchanged=True,owned_groups_closed=not bool(S.OWNED_OPEN),sealed_witnesses_unchanged=True,sealed_final_count=len(protected)+len(new_frozen),actual_executable_guards_passed=bool(new_frozen));assert not S.OWNED_OPEN
  except BaseException as error:terminal.update(status='FAIL',final_guard_error=repr(error))
  save('guard-events.json',guard_events);terminal['guard_policy']=P['guard_policy'];terminal['convenience_alias_inode_events']=alias_events;save('terminal.json',terminal)
 return 0 if terminal['status']=='PASS' else 1
if __name__=='__main__':raise SystemExit(main())
