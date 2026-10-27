"""Disabled full Skills qualification; current a17 source, closed failed-Updater integrity handoff only."""
import pathlib,json,hashlib,subprocess,os,sys,importlib.util,shutil,stat,re,tomllib,collections
sys.dont_write_bytecode=True
E=pathlib.Path(__file__).resolve().parent;P=json.loads((E/'plan.json').read_text())
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def save(n,v):
 assert n not in P['reserved_input_names']
 with (E/n).open('x') as f:json.dump(v,f,indent=2);f.write('\n');f.flush();os.fsync(f.fileno())
def bound(b):
 assert b and b['sha256'];p=pathlib.Path(b['path']);assert p.resolve().is_relative_to(E.parent) and sha(p)==b['sha256'] and ('bytes' not in b or p.stat().st_size==b['bytes']);return json.loads(p.read_text())
def attrs(p):
 p=pathlib.Path(p);s=p.lstat();return dict(bytes=s.st_size,mode=stat.S_IMODE(s.st_mode),mtime_ns=s.st_mtime_ns,device=s.st_dev,inode=s.st_ino,kind='symlink' if p.is_symlink() else 'file')
def binding(p):return {'sha256':sha(p),**attrs(p)}
def artifacts(raw):
 rows=[json.loads(l) for l in raw.splitlines()];end=[r for r in rows if r.get('reason')=='build-finished'];assert len(end)==1 and end[0]['success'];return [r for r in rows if r.get('reason')=='compiler-artifact']
def main():
 assert P['execution_ready'] and P['canonical_alias_bound'] and P['compiler_lease']=='root-skills-current-body-full'
 L=json.loads((E/'launch.json').read_text());assert L['controller_sha256']==sha(__file__) and L['plan_sha256']==sha(E/'plan.json') and L['compiler_lease']==P['compiler_lease'] and L['require_escalated'] and L['root_review_approved'] and L['exclusive_source'] and L['exclusive_target'] and L['parent_closed_and_released'] and L['concurrency_context']['idle_host_claim'] is False
 H=bound(P['handoff_binding']);release=bound(H['release_receipt']);parent=bound(H['parent_terminal']);native=bound(H['parent_native']);rust=bound(H['parent_Rust_command']);assert H['status']=='ROOT_VERIFIED_INTEGRITY_RELEASE_AFTER_FAILED_UPDATER' and parent['status']=='FAIL' and native['exit_code']==native['controller_exit']==native['wrapper_exit']==1 and rust['exit_code']==rust['direct_wait_status']==101
 assert H['retained_parent_outcome']=={'status':'FAIL','native_exit':1,'Rust_exit':101,'parser_KeyError_retained':True,'offline_reconciliation_not_a_pass':True} and parent['error']=="KeyError('test_rows')" and L['independent_gate_after_failed_parent_authorized'] and parent['lease_released'] and parent['unresolved_owned_pids']==[] and all(parent[k] for k in ['source_unchanged','sealed_guard_pass','actual_guard_pass']) and rust['owned_group_absent_after_cleanup'] and not rust['timed_out'] and not rust['disk_reserve_breached']
 previous_source=bound(H['parent_source_before']);assert previous_source==bound(H['parent_source_after']) and previous_source['head']==parent['head']
 protected=bound(H['protected_manifest']);retired=bound(H['mutable_manifest']);verified=bound(H['verified_bindings']);assert len(protected)==H['sealed_count']==parent['sealed_count']==release['protected_count']==P['inherited_sealed_count'] and len(retired)==H['mutable_count']==release['actual_count']==P['retired_parent_mutable_count'] and set(protected)==set(verified['sealed']) and set(retired)==set(verified['actual'])
 for field,value in H['raw_release_true_fields'].items():assert value is True and release[field] is True
 for label,rows in [('sealed',protected),('actual',retired)]:
  for n,row in rows.items():assert all(row[k]==verified[label][n][k] for k in ['sha256','bytes','mode','mtime_ns','device','inode'])
 for item in [H['lsof_stdout'],H['lsof_stderr']]:assert item['bytes']==0 and pathlib.Path(item['path']).stat().st_size==0 and sha(item['path'])==item['sha256']
 W,T,O=map(pathlib.Path,[P['worktree'],P['target'],P['official_worktree']]);assert W!=O and release['status']=='INTEGRITY_RELEASE_AFTER_FAILED_UPDATER_HOST_GATE' and release['previous']=='3843-1d97-full-affected-gates-prep' and release['parent_native_exit']==1 and release['parent_Rust_exit']==101 and release['original_parser_KeyError_FAIL_retained'] and release['offline_reconciliation_not_a_pass'] and release['target']==str(T) and release['lsof_exit']==1
 assert L['allocation']=={'kind':'Skills','head':P['head'],'gate':str(E),'worktree':str(W),'target':str(T)} and P['target_dev_ino']==L['target_dev_ino']==H['target_dev_ino']==release['target_dev_ino'] and T.is_dir() and not T.is_symlink() and [T.stat().st_dev,T.stat().st_ino]==P['target_dev_ino']
 # The Root release already retired these102 bindings; only the132 independent copies remain active.
 prior={}
 history=bound(P['retained_failure_census']);assert history['counts']=={'passed':521,'failed':1,'ignored':9} and history['command_exit']==101 and bound(P['retained_failure_terminal'])['status']=='FAIL'
 for item in P['historical_receipts'].values():assert sha(item['path'])==item['sha256']
 imported=bound(P['canonical_import_receipt']);assert imported['status']=='OWN_QUALIFIER_ALIAS_IN_INDEPENDENT_STORE' and imported['head']==P['head'] and imported['alias']==P['canonical_branch'] and imported['only_owned_local_branches_and_worktrees_changed'] and imported['origin_tracking_ref_refreshed_as_requested'] and imported['source_git_store']==P['official_git_store']
 official=bound(P['official_source_binding']);initial=bound(P['canonical_source_binding']);assert initial['head']==official['head']==P['head'] and initial['status']==official['status']=='' and initial['branch']==P['canonical_branch'] and official['branch']==P['official_branch'] and {k:v for k,v in initial.items() if k!='branch'}=={k:v for k,v in official.items() if k!='branch'}
 assert sha(E/'support.py')==P['support_sha256'];sp=importlib.util.spec_from_file_location('skills_full_supervisor',E/'support.py');S=importlib.util.module_from_spec(sp);sp.loader.exec_module(S);Q=S.Q;Q.W=W
 def git(root,*a):return subprocess.check_output(['git','--no-optional-locks','-C',str(root),*a])
 def source_stats(root,expected):
  assert git(root,'rev-parse','HEAD').decode().strip()==P['head'] and git(root,'rev-parse','HEAD^').decode().strip()==P['base'] and git(root,'branch','--show-current').decode().strip()==expected['branch'] and git(root,'status','--porcelain=v1').decode()==''
  idx=git(root,'ls-files','--stage','-z');names={os.fsdecode(x.split(b'\t',1)[1]) for x in idx.split(b'\0') if x};assert names==set(expected['files']) and hashlib.sha256(idx).hexdigest()==expected['index_sha256'];return {n:attrs(root/n) for n in sorted(names)}
 physical=source_stats(W,initial);owner_physical=source_stats(O,official);assert physical==bound(P['canonical_source_stat_binding']) and owner_physical==bound(P['official_source_stat_binding']);frozen={};actual={};aliases=set();alias_events=[];consumed=[];hash_points=[];terminal={'status':'FAIL','head':P['head'],'base':P['base'],'runtime_attempts':0,'scope':'Fresh full unfiltered serial Skills Cargo package including docs/custom cases','runtime_reuse':False,'old_521_1_9_failure_retained':True,'no_clean':True,'full_reverse_package_runtime_claim':False,'native_claim':False}
 def guard(full=False,allow_alias=False):
  assert source_stats(W,initial)==physical and source_stats(O,official)==owner_physical and not T.is_symlink() and [T.stat().st_dev,T.stat().st_ino]==P['target_dev_ino']
  for n,b in {**protected,**frozen}.items():
   q=pathlib.Path(n);a=attrs(q);assert q.is_file() and not q.is_symlink() and q.resolve().is_relative_to(E.parent) and not q.resolve().is_relative_to(T) and not q.resolve().is_relative_to(W) and all(a[k]==b[k] for k in a if k in b)
   if full:assert sha(q)==b['sha256']
  for n,b in {**prior,**actual}.items():
   q=pathlib.Path(n);a=attrs(q);assert q.is_file() and not q.is_symlink() and q.resolve().is_relative_to(T)
   if allow_alias and n in aliases and a['inode']!=b['inode']:
    assert all(a[k]==b[k] for k in a if k!='inode') and sha(q)==b['sha256'];alias_events.append({'path':n,'old_inode':b['inode'],'new_inode':a['inode'],'all_other_fields_SHA_equal':True});actual[n]={**b,**a}
   else:assert all(a[k]==b[k] for k in a if k in b)
   assert sha(q)==b['sha256']
  if full:
   assert Q.snapshot(set())==initial;Q.W=O
   try:assert Q.snapshot(set())==official
   finally:Q.W=W
   hash_points.append('initial' if not hash_points else 'final')
 def consume(paths,phase):
  for n in paths:
   assert sha(n)==actual[n]['sha256'];copy=actual[n]['sealed_copy'];assert sha(copy)==frozen[copy]['sha256']
  consumed.append({'phase':phase,'paths':paths,'actual_and_independent_copy_SHA_verified':True})
 env=os.environ.copy()
 for k in list(env):
  if k.startswith(('LAMP_','CARGO_PROFILE_','CARGO_TARGET_')) or k in ['CARGO_BUILD_TARGET','RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','RUSTC_WRAPPER','RUSTC_WORKSPACE_WRAPPER','RUSTUP_TOOLCHAIN','RUST_TEST_THREADS']:env.pop(k)
 env.update(CARGO_TARGET_DIR=str(T),CARGO_BUILD_JOBS='1',CARGO_INCREMENTAL='0',CARGO_PROFILE_DEV_DEBUG='0',CARGO_PROFILE_TEST_DEBUG='0',CARGO_NET_OFFLINE='true',CARGO_TERM_COLOR='never');env['PATH']='/Users/autonomous/.cargo/bin:'+env.get('PATH','')
 def command(label,args,cargo=True,allowed=(0,),timeout=3600,postguard=True,paths=None):
  assert not S.OWNED_OPEN
  if cargo:assert args[0] in ['fmt','metadata','clippy','build','test'] and (args[0]!='test' or '--no-run' in args or label=='full-skills' and args==['test','--offline','--locked','--jobs','1','-p','lamp-skills','--no-fail-fast','--','--test-threads=1'])
  else:assert args[0] in drivers and args[1:] in [['--list','--format','terse'],['--ignored','--list','--format','terse']]
  guard();consume(paths,label+'-before') if paths else None;argv=['/Users/autonomous/.cargo/bin/cargo','+1.96.0',*args] if cargo else args;r=S.run_command(argv,W/'lampOS',env,E,'cmd-'+label,timeout)
  if postguard:guard();consume(paths,label+'-after') if paths else None
  assert r['exit_code'] in allowed and r['direct_wait_status']==r['exit_code'] and r['owned_group_absent_after_cleanup'] and not r['unresolved_owned_pids'] and not r['timed_out'] and not r['disk_reserve_breached'] and not r.get('unexpected_owned_descendants');return r,(E/r['stdout']).read_text(),(E/r['stderr']).read_text()
 def bindings(rows,label):
  local=[];registry=[]
  for a in rows:
   p=packages[a['package_id']];assert pathlib.Path(a['manifest_path']).resolve()==pathlib.Path(p['manifest_path']).resolve() and a['target'] in p['targets'] and isinstance(a['fresh'],bool) and set(a['features'])<=set(p['features'])
   if p['source'] is not None:registry.append({'artifact':a,'resolved_source':p['source'],'version':p['version']});continue
   root=pathlib.Path(p['manifest_path']).resolve().parent;assert root.is_relative_to(W) and pathlib.Path(a['target']['src_path']).resolve().is_relative_to(root) and a['profile']['debuginfo']==0 and a['profile']['debug_assertions'] and a['profile']['overflow_checks'];prefix=root.relative_to(W).as_posix()+'/';inputs={n:b for n,b in initial['files'].items() if n.startswith(prefix)};assert inputs;local.append({'artifact':a,'tracked_package_inputs':inputs})
  assert any(a['package_id']==package['id'] for a in rows);save(label+'-source-profile-bindings.json',{'local':local,'registry':registry,'lock':initial['files']['lampOS/Cargo.lock'],'no_registry_byte_or_transitive_freshness_claim':True})
 try:
  guard(True);save('source-before.json',initial);save('official-source-before.json',official);save('source-stat-before.json',physical);save('official-stat-before.json',owner_physical);save('protected-before.json',protected);save('parent-outcome-retained.json',H['retained_parent_outcome']);save('environment-hashes.json',{k:hashlib.sha256(v.encode()).hexdigest() for k,v in env.items()})
  previous=bound(H['parent_source_files']);assert previous==previous_source['files'];changes={n:{'before':previous.get(n),'after':initial['files'].get(n)} for n in sorted(set(previous)|set(initial['files'])) if previous.get(n)!=initial['files'].get(n)};transition=bound(P['parent_source_transition']);assert changes==transition['changed_entries'] and transition['child_snapshot']==P['canonical_source_binding'] and transition['parent_snapshot']==H['parent_source_before'];save('parent-source-transition-observed.json',{'changed_entries':changes,'no_parent_runtime_transfer':True})
  routine=bound(P['Routine_3598_scope']);assert initial['files']['lampOS/crates/skills/src/routine.rs']['sha256']==routine['source_sha256'] and routine['stock_declaration_ms']==500 and routine['clock_policy_fixture_override_ms']==5000 and routine['historical_cause_not_established'];save('current-Routine3598-scope.json',routine)
  for root in [W,O]:
   for rel in [P['changed_test_input'],'lampOS/crates/skills/src/routine.rs']:assert not any(marker.encode() in (root/rel).read_bytes() for marker in P['known_forbidden_diagnostic_markers'])
  command('fmt',['fmt','--all','--check'],timeout=120);_,raw,_=command('metadata',['metadata','--offline','--locked','--format-version','1'],timeout=120);m=json.loads(raw);assert pathlib.Path(m['workspace_root']).resolve()==W/'lampOS' and pathlib.Path(m['target_directory']).resolve()==T;save('actual-metadata.json',m);packages={p['id']:p for p in m['packages']};package=next(p for p in m['packages'] if p['name']=='lamp-skills');affected={package['id']}
  while True:
   wider=affected|{n['id'] for n in m['resolve']['nodes'] if any(d['pkg'] in affected for d in n['deps'])}
   if wider==affected:break
   affected=wider
  closure=sorted(packages[i]['name'] for i in affected&set(m['workspace_members']));save('actual-reverse-closure.json',{'seed':'lamp-skills','all_resolved_edge_kinds':True,'packages':closure});flags=[v for n in closure for v in ['-p',n]]
  guard();save('retired-parent-mutable-bindings.json',{'bindings':retired,'all_SHA_metadata_verified_before_retirement':True,'no_clean':True,'independent_sealed_count':len(protected)});prior={}
  _,raw,_=command('strict',['clippy','--offline','--locked','--all-targets','--jobs','1','--message-format=json',*flags,'--','-D','warnings']);rows=artifacts(raw);save('strict-artifacts.json',rows);bindings(rows,'strict')
  _,raw,_=command('workspace',['build','--offline','--locked','--workspace','--jobs','1','--message-format=json']);rows=artifacts(raw);save('workspace-artifacts.json',rows);bindings(rows,'workspace')
  common=['test','--offline','--locked','--jobs','1','-p','lamp-skills'];_,raw,_=command('test-compile',[*common,'--no-run','--message-format=json']);rows=artifacts(raw);save('test-compiler-artifacts.json',rows);bindings(rows,'test-compile');exes={}
  for a in rows:
   if a.get('executable'):
    assert a['package_id']==package['id'] and pathlib.Path(a['executable']).resolve().is_relative_to(T)
    if a['executable'] in exes:assert exes[a['executable']]==a
    exes[a['executable']]=a
  manifest=tomllib.loads(pathlib.Path(package['manifest_path']).read_text());custom={t['name'] for t in manifest.get('test',[]) if t.get('harness') is False};drivers={n:a for n,a in exes.items() if a['profile']['test'] or a['target']['kind']==['test'] and a['target']['name'] in custom};expected={(t['name'],tuple(t['kind'])) for t in package['targets'] if t['test'] and t['kind']!=['example']};assert {(a['target']['name'],tuple(a['target']['kind'])) for a in drivers.values()}==expected
  normal={n:a for n,a in exes.items() if not a['profile']['test'] and a['target']['kind'] in [['bin'],['example']]};expected_normal={(t['name'],tuple(t['kind'])) for t in package['targets'] if t['kind'] in [['bin'],['example']]};assert {(a['target']['name'],tuple(a['target']['kind'])) for a in normal.values()}==expected_normal and set(exes)==set(drivers)|set(normal);assert all(pathlib.Path(n)==(T/'debug'/a['target']['name'] if a['target']['kind']==['bin'] else T/'debug'/'examples'/a['target']['name']) for n,a in normal.items());aliases=set(normal);folder=E/'frozen';folder.mkdir()
  for i,(n,a) in enumerate(exes.items()):
   b=binding(n);assert min(shutil.disk_usage(E).free,shutil.disk_usage(T).free)>P['operational_reserve_bytes']+b['bytes'];f=folder/(str(i)+'-'+pathlib.Path(n).name);shutil.copy2(n,f);fb=binding(f);assert fb['sha256']==b['sha256'] and (fb['device'],fb['inode'])!=(b['device'],b['inode']);frozen[str(f)]=fb;actual[n]={**b,'artifact':a,'sealed_copy':str(f)}
  save('actual-executables.json',actual);save('new-frozen.json',frozen);save('derived-targets.json',{'test_drivers':list(drivers.values()),'normal_program_helpers':list(normal.values()),'custom_targets':sorted(custom),'all_counts_metadata_derived':True})
  changed=[a for a in drivers.values() if pathlib.Path(a['target']['src_path']).resolve()==W/'lampOS/crates/skills/tests/it/main.rs'];assert len(changed)==1 and changed[0]['fresh'] is False
  import shlex
  dep=pathlib.Path(changed[0]['executable']).with_suffix('.d');assert dep.is_file();line=next(l for l in dep.read_text().replace('\\\n','').splitlines() if ': ' in l);reached={}
  for name in shlex.split(line.split(': ',1)[1]):
   q=pathlib.Path(name);q=q if q.is_absolute() else W/'lampOS'/q
   if q.resolve().is_relative_to(W):
    rel=q.resolve().relative_to(W).as_posix();assert rel in initial['files'] and sha(q)==initial['files'][rel]['sha256'];reached[rel]=initial['files'][rel]
  assert P['changed_test_input'] in reached;save('changed-IT-binding.json',{'artifact':changed[0],'depinfo_sha256':sha(dep),'actual_local_inputs':reached,'fresh_false_selected_only':True})
  for n in actual:
   carry=b'';markers=[x.encode() for x in P['known_forbidden_diagnostic_markers']]
   with pathlib.Path(n).open('rb') as f:
    while chunk:=f.read(1024*1024):
     data=carry+chunk;assert not any(x in data for x in markers),'Known private diagnostic marker';carry=data[-max(map(len,markers)):]
  inventory={};census=[]
  for i,(n,a) in enumerate(drivers.items()):
   inv={}
   for which,opts in [('all',[]),('ignored',['--ignored'])]:
    _,raw,_=command('inventory-'+str(i)+'-'+which,[n,*opts,'--list','--format','terse'],False,timeout=300,paths=[n]);inv[which]=sorted(l.removesuffix(': test') for l in raw.splitlines() if l.endswith(': test'));assert len(inv[which])==len(set(inv[which]))
   assert set(inv['ignored'])<=set(inv['all']);inventory[n]=inv;census.append({'package':'lamp-skills','target':a['target'],'executable':n,'sha256':actual[n]['sha256'],'compiled_cases':inv['all'],'compiled_ignored_cases':inv['ignored'],'custom_entrypoint':a['target']['name'] in custom})
  assert all(sum(inv['all'].count(case) for inv in inventory.values())==1 and not any(case in inv['ignored'] for inv in inventory.values()) for case in P['required_cases']);save('inventory.json',inventory);save('compiled-census.json',census);save('derived-inventory-counts.json',{'all':sum(len(v['all']) for v in inventory.values()),'ignored':sum(len(v['ignored']) for v in inventory.values()),'no_historical_count_used':True})
  assert sha(P['census_parser_path'])==P['census_parser_sha256'];sys.modules['support']=S;sp=importlib.util.spec_from_file_location('skills_full_cargo_census',P['census_parser_path']);C=importlib.util.module_from_spec(sp);sp.loader.exec_module(C);docs=['lamp-skills'] if any(t['kind']==['lib'] and t.get('doctest') for t in package['targets']) else [];terminal['runtime_attempts']=1;r=None;result=None
  try:r,out,err=command('full-skills',[*common,'--no-fail-fast','--','--test-threads=1'],allowed=(0,101),timeout=P['full_runtime_timeout_s'],postguard=False,paths=list(actual))
  finally:
   q=E/'cmd-full-skills.stdout.txt';raw=q.read_text() if q.exists() else '';save('raw-Rust-statuses-before-guards.json',{'rows':re.findall(r'^test (.+?) \.\.\. (ok|FAILED|ignored)(?:,.*)?$',raw,re.M),'summaries':re.findall(r'^test result: .*$',raw,re.M),'scope':'All raw partial status lines, including possible diagnostic reprints; not an execution census. Target-bound census follows','runtime_attempts':1});record_path=E/'cmd-full-skills.json'
   if record_path.exists():
    try:
     assert not (E/'target-outcomes.json').exists();result=C.target_outcomes(json.loads(record_path.read_text()),E,census,['lamp-skills'],docs,W);save('observed-census-before-guards.json',result)
    except BaseException as error:terminal['runtime_parser_error']=repr(error);save('runtime-parser-error-before-guards.json',{'error':repr(error),'no_Rust_failure_or_unrun_waived':True})
  valid=r['exit_code'] in [0,101] and r['owned_group_absent_after_cleanup'] and not re.search(r'^\s*(?:Compiling|Checking)\s',out+'\n'+err,re.M);guard(allow_alias=valid);consume(list(actual),'full-skills-after');save('normal-alias-inode-events.json',alias_events);groups=[t for t in (result or {}).get('targets',[]) if t['kind']=='executable'];counts={k:sum(t.get('counts',{}).get(k,0) for t in groups) for k in ['passed','failed','ignored']};save('derived-runtime-counts.json',{'counts':counts,'doc_targets':[t for t in (result or {}).get('targets',[]) if t['kind']=='doctests'],'Rust_exit':r['exit_code'],'runtime_reuse':False,'ordinary_filters':0})
  terminal.update(observed_counts=counts,runtime_command_exit=r['exit_code'],complete_target_binding=bool(result and result['complete_binding']));assert result and result['complete_binding'] and all('test_rows' in t for t in groups) and r['exit_code']==0 and counts['failed']==0 and not any(t.get('counts',{}).get('failed') for t in result['targets']);assert all(sum(n==case and status=='ok' for t in groups for n,status in t['test_rows'])==1 for case in P['required_cases']);terminal.update(status='PASS',counts=counts,required_cases_once=P['required_cases'],single_addon_Rust_identity_with_two_internal_variants=True,strict_packages=closure,actual_driver_count=len(drivers),actual_helper_count=len(normal),doc_targets=docs)
 except BaseException as error:terminal['error']=repr(error)
 finally:
  try:guard(True);save('source-after.json',Q.snapshot(set()));save('official-source-after.json',official);save('protected-after.json',{**protected,**frozen});save('actual-after.json',actual);terminal.update(source_unchanged=True,official_owner_unchanged=True,sealed_and_actual_guards=True,protected_count=len(protected)+len(frozen),actual_count=len(actual),full_hash_checkpoints=hash_points)
  except BaseException as error:terminal.update(status='FAIL',guard_error=repr(error))
  terminal.update(owned_groups_closed=not bool(S.OWNED_OPEN),unresolved_owned_pids=sorted(S.OWNED_OPEN),lease_released=not S.OWNED_OPEN);save('consumed-SHA-checkpoints.json',consumed);save('terminal.json',terminal)
 return 0 if terminal['status']=='PASS' else 1
if __name__=='__main__':raise SystemExit(main())
