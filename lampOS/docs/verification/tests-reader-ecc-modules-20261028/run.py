"""Disabled Sentinel Reader and service-trace module gate; future first released cache/source alias stay deferred; committed current source."""
import pathlib,json,hashlib,subprocess,os,sys,importlib.util,shutil,stat,re,shlex,collections
sys.dont_write_bytecode=True
E=pathlib.Path(__file__).resolve().parent;P=json.loads((E/'plan.json').read_text())
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def save(n,v):
 assert n not in P['input_names_reserved']
 with (E/n).open('x') as f:json.dump(v,f,indent=2);f.write('\n');f.flush();os.fsync(f.fileno())
def bound(item):
 assert item and item['sha256'];q=pathlib.Path(item['path']);assert q.resolve().is_relative_to(E.parent) and sha(q)==item['sha256'] and ('bytes' not in item or q.stat().st_size==item['bytes']);return json.loads(q.read_text())
def attrs(p):
 p=pathlib.Path(p);s=p.lstat();return dict(bytes=s.st_size,mode=stat.S_IMODE(s.st_mode),mtime_ns=s.st_mtime_ns,device=s.st_dev,inode=s.st_ino,kind='symlink' if p.is_symlink() else 'file')
def binding(p):return dict(sha256=sha(p),**attrs(p))
def cargo_artifacts(raw):
 rows=[json.loads(l) for l in raw.splitlines()];finished=[r for r in rows if r.get('reason')=='build-finished'];assert len(finished)==1 and finished[0]['success'];return [r for r in rows if r.get('reason')=='compiler-artifact']
def main():
 assert P['execution_ready'] and P['source_carry_verified'] and P['canonical_alias_bound'] and P['compiler_lease']=='root-reader-a17-first-released-cache' and P['controller_sha256']==sha(__file__)
 L=json.loads((E/'launch.json').read_text());assert L['require_escalated'] and L['root_review_approved'] and L['exclusive_target'] and L['parent_closed_and_target_released'] and L['exclusive_canonical_source'] and L['exclusive_official_source'] and L['compiler_lease']==P['compiler_lease'];assert L['controller_sha256']==sha(__file__) and L['plan_sha256']==sha(E/'plan.json') and L['concurrency_context']['idle_host_claim'] is False
 assert sha(E/'support.py')==P['support_sha256'];sp=importlib.util.spec_from_file_location('sentinel_full_supervisor',E/'support.py');S=importlib.util.module_from_spec(sp);sp.loader.exec_module(S);Q=S.Q
 W,T,O=map(pathlib.Path,[P['worktree'],P['target'],P['official_worktree']]);assert W!=O and sys.platform=='darwin';Q.W=W
 H=bound(P['handoff_binding']);assert H['status']=='ROOT_REVIEWED_CLOSED_RELEASE_FOR_READER' and H['released_for_head']==P['head']
 assert H['release_receipt']['path']==P['handoff_release_path'];release=bound(H['release_receipt']);parent=bound(H['parent_terminal']);native=bound(H['parent_native']);before=bound(H['parent_source_before']);after=bound(H['parent_source_after'])
 assert parent['status']==H['retained_parent_outcome']['status'] and native[H['native_exit_field']]==H['retained_parent_outcome']['native_exit'] and before==after and parent['lease_released'] and parent['unresolved_owned_pids']==[] and H['source_guard_fields'] and all(parent[k] is True for k in H['source_guard_fields'])
 if H['retained_parent_outcome']['native_exit']!=0 or parent['status']!='PASS':assert L['independent_gate_after_failed_parent_authorized']
 assert release['status']==H['raw_release_status'] and release['target']==str(T) and release['target_dev_ino']==P['target_dev_ino'] and release['lsof_exit']==1
 assert set(H['raw_release_true_fields'])=={'sealed_verified','actual_verified','actual_retired','lsof_closed','groups_closed','no_failure_waiver'} and all(release[field] is True for field in H['raw_release_true_fields'].values())
 for item in [H['release_lsof_stdout'],H['release_lsof_stderr']]:assert item['bytes']==0 and sha(item['path'])==item['sha256'] and pathlib.Path(item['path']).stat().st_size==0
 assert len(H['parent_command_receipts'])==len(H['parent_command_expected_exits'])>0
 for item,expected_exit in zip(H['parent_command_receipts'],H['parent_command_expected_exits']):
  r=bound(item);assert r['exit_code']==r['direct_wait_status']==expected_exit and not r['timed_out'] and not r['disk_reserve_breached'] and r['owned_group_absent_after_cleanup'] and not r['unresolved_owned_pids'] and not r.get('unexpected_owned_descendants')
 protected=bound(H['sealed_manifest']);retired=bound(H['retired_mutable_manifest']);verified=bound(H['release_verified_bindings']);assert len(protected)==P['expected_inherited_sealed_count']==H['sealed_count']==parent[H['parent_protected_count_field']]==release[H['raw_release_count_fields']['sealed']] and len(retired)==P['expected_retired_mutable_count']==H['retired_mutable_count']==release[H['raw_release_count_fields']['actual']] and set(protected)==set(verified['sealed']) and set(retired)==set(verified['actual'])
 keys=['sha256','bytes','mode','mtime_ns','device','inode']
 for n,v in protected.items():assert all(v[k]==verified['sealed'][n][k] for k in keys)
 assert H['mutable_schema'] in ['direct','bound_observed']
 for n,row in retired.items():
  v=row if H['mutable_schema']=='direct' else row['observed'];assert all(v[k]==verified['actual'][n][k] for k in keys)
  if H['mutable_schema']=='bound_observed':assert all(row['bound'][k]==v[k] for k in keys)
 for item in H['retained_prior_receipts'].values():bound(item)
 assert P['target_dev_ino']==H['target_dev_ino']==L['allocation']['target_dev_ino'] and T.is_dir() and not T.is_symlink() and [T.stat().st_dev,T.stat().st_ino]==P['target_dev_ino'] and L['allocation']=={'head':P['head'],'worktree':str(W),'target':str(T),'target_dev_ino':P['target_dev_ino'],'gate':str(E)}
 initial=bound(P['source_binding']);official=bound(P['official_source_binding']);assert initial['head']==official['head']==P['head'] and initial['status']==official['status']==P['expected_status'];assert initial['branch']==P['branch'] and official['branch']==P['official_branch'];assert {k:v for k,v in initial.items() if k!='branch'}=={k:v for k,v in official.items() if k!='branch'}
 assert P['expected_status']=='' and P['approved_dirty_paths']==[] and P['approved_payloads']=={}
 def git(root,*a):return subprocess.check_output(['git','--no-optional-locks','-C',str(root),*a])
 def source_stats(root,expected,branch):
  assert git(root,'rev-parse','HEAD').decode().strip()==P['head'] and git(root,'rev-parse','HEAD^').decode().strip()==P['base'] and git(root,'branch','--show-current').decode().strip()==branch and git(root,'status','--porcelain=v1').decode()==P['expected_status']
  idx=git(root,'ls-files','--stage','-z');assert hashlib.sha256(idx).hexdigest()==expected['index_sha256'];names={os.fsdecode(r.split(b'\t',1)[1]) for r in idx.split(b'\0') if r};assert names==set(expected['files']);return {n:attrs(root/n) for n in sorted(names)}
 canonical_physical=source_stats(W,initial,P['branch']);official_physical=source_stats(O,official,P['official_branch']);assert canonical_physical==bound(P['canonical_physical_binding']) and official_physical==bound(P['official_physical_binding']);frozen={};actual={};module=None;consumed=[];hash_points=[];terminal={'status':'FAIL','head':P['head'],'base':P['base'],'runtime_attempts':0,'runtime_scope':P['runtime_scope'],'source_state':'clean committed current own Reader head','committed_head_alone_qualification_claim':True,'Linux_reader_cases_run':False,'old_mutable_bindings_retired_count':len(retired),'no_clean':True}
 epoch=bound(P['cache_epoch_binding']);ep=P['required_cache_epoch_path'];es=(W/ep).stat();assert epoch['path']==ep and epoch['head']==P['head'] and epoch['canonical_root']==str(W) and epoch['canonical_source_checkout_complete'] is True and epoch['source_index_sha256']==initial['index_sha256'] and epoch['bytes_sha256']==initial['files'][ep]['sha256'] and epoch['mtime_ns']==es.st_mtime_ns and epoch['mtime_refresh_performed'] is False
 def witnesses(full=False):
  for n,b in {**protected,**frozen}.items():
   q=pathlib.Path(n);a=attrs(q);assert q.is_file() and not q.is_symlink() and q.resolve().is_relative_to(E.parent) and not q.resolve().is_relative_to(T) and not q.resolve().is_relative_to(W);assert all(a[k]==b[k] for k in a if k in b),n
   if full:assert sha(q)==b['sha256'],n
 def guard(full=False,phase=None,consume=None):
  assert source_stats(W,initial,P['branch'])==canonical_physical and source_stats(O,official,P['official_branch'])==official_physical;assert not T.is_symlink() and [T.stat().st_dev,T.stat().st_ino]==P['target_dev_ino'];witnesses(full)
  for n,b in actual.items():
   q=pathlib.Path(n);a=attrs(q);assert q.is_file() and not q.is_symlink() and q.resolve().is_relative_to(T) and all(a[k]==b[k] for k in a) and sha(q)==b['sha256'],n
  if full:
   assert Q.snapshot(set(P['approved_dirty_paths']))==initial;Q.W=O
   try:assert Q.snapshot(set(P['approved_dirty_paths']))==official
   finally:Q.W=W
   hash_points.append(phase)
  if consume:
   for n in consume:
    b=actual[str(n)];assert sha(n)==b['sha256'];copy=pathlib.Path(b['sealed_copy']);assert sha(copy)==frozen[str(copy)]['sha256']
   consumed.append({'phase':phase,'actual_and_new_sealed_SHA':True,'executables':list(map(str,consume)),'all_inherited_frozen_SHA_rehash':False})
 env=os.environ.copy()
 for k in list(env):
  if k.startswith(('LAMP_','CARGO_PROFILE_','CARGO_TARGET_')) or k in ['CARGO_BUILD_TARGET','RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','RUSTC_WRAPPER','RUSTC_WORKSPACE_WRAPPER','RUSTUP_TOOLCHAIN','RUST_TEST_THREADS']:env.pop(k)
 env.update(CARGO_TARGET_DIR=str(T),CARGO_BUILD_JOBS='1',CARGO_INCREMENTAL='0',CARGO_PROFILE_DEV_DEBUG='0',CARGO_PROFILE_TEST_DEBUG='0',CARGO_NET_OFFLINE='true',CARGO_TERM_COLOR='never');env['PATH']='/Users/autonomous/.cargo/bin:'+env.get('PATH','')
 def command(label,args,timeout=3600,consume=None,allowed=(0,),postguard=True,defer_validation=False):
  assert not S.OWNED_OPEN;guard(phase=label+'-before',consume=consume);r=S.run_command(args,W/'lampOS',env,E,label,timeout)
  if postguard:guard(phase=label+'-after',consume=consume)
  if not defer_validation:
   if r.get('exit_code')==0:S.require_success(r)
   else:assert r.get('exit_code') in allowed and r['direct_wait_status']==r['exit_code'] and not r['timed_out'] and not r['disk_reserve_breached'] and r['owned_group_absent_after_cleanup'] and not r['unresolved_owned_pids'] and not r.get('unexpected_owned_descendants')
  return r,(E/r['stdout']).read_text()
 def local_bindings(rows,label):
  out=[]
  for a in rows:
   p=packages[a['package_id']]
   if p['source'] is not None:continue
   root=pathlib.Path(p['manifest_path']).parent;assert root.resolve().is_relative_to(W) and pathlib.Path(a['manifest_path']).resolve()==pathlib.Path(p['manifest_path']).resolve() and a['target'] in p['targets'] and isinstance(a['fresh'],bool);assert a['profile']['debuginfo']==0 and set(a['features'])<=set(p['features']);inputs={n:v for n,v in initial['files'].items() if n.startswith(root.relative_to(W).as_posix()+'/')};assert inputs;out.append({'artifact':a,'all_tracked_package_inputs':inputs})
  assert any(a['artifact']['package_id']==voice['id'] for a in out);save(label+'-source-profile-bindings.json',{'artifacts':out,'freshness_policy':'All observed only except selected VoiceLIB required fresh:false'})
 def freeze(a,label):
  q=pathlib.Path(a['executable']);assert q.resolve().is_relative_to(T) and not q.is_symlink();b=binding(q);assert shutil.disk_usage(E).free>P['operational_reserve_bytes']+b['bytes'];f=E/'frozen'/f"{label}-{b['sha256'][:16]}-{q.name}";shutil.copy2(q,f);fb=binding(f);assert fb['sha256']==b['sha256'] and (fb['device'],fb['inode'])!=(b['device'],b['inode']);frozen[str(f)]=fb;actual[str(q)]={**b,'artifact':a,'sealed_copy':str(f)};return q
 C=['/Users/autonomous/.cargo/bin/cargo','+1.96.0'];flags=['--offline','--locked','--jobs','1']
 try:
  guard(full=True,phase='initial');save('source-before.json',initial);save('official-source-initial.json',official);save('canonical-stat-initial.json',canonical_physical);save('official-stat-initial.json',official_physical);save('protected-before.json',protected);save('retired-parent-mutable-bindings.json',{'manifest':H['retired_mutable_manifest'],'count':len(retired),'all_declared_retired_by_closed_root_handoff':True,'none_guarded_as_active_current_paths':True});save('retained-parent-history.json',H['retained_prior_receipts']);save('environment-hashes.json',{k:hashlib.sha256(v.encode()).hexdigest() for k,v in env.items()})
  previous=before[H['parent_files_key']] if H['parent_files_key'] is not None else before
  semantic=lambda row:{k:row[k] for k in ['sha256','bytes','mode','kind']};changes={n:{'before':semantic(previous[n]) if n in previous else None,'after':initial['files'].get(n)} for n in sorted(set(previous)|set(initial['files'])) if (semantic(previous[n]) if n in previous else None)!=initial['files'].get(n)};transition=bound(P['source_transition']);assert changes==transition['changed_entries'];save('parent-source-transition-observed.json',{'changed_entries':changes,'no_parent_runtime_transfer':True})
  for root in [W,O]:
   for rel in [P['required_cache_epoch_path'],P['immutable_stalled_control_source_path']]:assert not any(marker.encode() in (root/rel).read_bytes() for marker in P['known_forbidden_diagnostic_markers'])
  command('cmd-fmt',C+['fmt','--all','--check'],120)
  _,raw=command('cmd-metadata',C+['metadata','--offline','--locked','--format-version','1'],120);m=json.loads(raw);assert pathlib.Path(m['workspace_root']).resolve()==W/'lampOS' and pathlib.Path(m['target_directory']).resolve()==T;save('actual-metadata.json',m);packages={p['id']:p for p in m['packages']};voice=next(p for p in m['packages'] if p['name']=='lamp-voice');affected={voice['id']}
  while True:
   wider=affected|{n['id'] for n in m['resolve']['nodes'] if any(d['pkg'] in affected for d in n['deps'])}
   if wider==affected:break
   affected=wider
  names=sorted(packages[i]['name'] for i in affected&set(m['workspace_members']));assert 'lamp-voice' in names
  if P['strict_expected'] is not None:assert names==P['strict_expected']
  save('actual-reverse-closure.json',{'seed':'lamp-voice','all_resolved_edge_kinds':True,'packages':names});ps=[v for n in names for v in ['-p',n]]
  _,raw=command('cmd-strict',C+['clippy',*flags,'--all-targets','--message-format=json',*ps,'--','-D','warnings']);rows=cargo_artifacts(raw);save('strict-artifacts.json',rows);local_bindings(rows,'strict')
  _,raw=command('cmd-workspace',C+['build',*flags,'--workspace','--message-format=json']);rows=cargo_artifacts(raw);save('workspace-artifacts.json',rows);local_bindings(rows,'workspace');(E/'frozen').mkdir();helpers=[a for a in rows if a['target']['name'] in P['runtime_helper_target_names'] and a.get('executable') and not a['profile']['test']];assert {a['target']['name'] for a in helpers}==set(P['runtime_helper_target_names'])
  helper_phase={a['executable']:binding(a['executable']) for a in helpers};save('workspace-helper-phase-observations.json',{'bindings':helper_phase,'not_yet_active_current_guards':True,'next_compiler_phase_has_no_helper_consumption':True})
  _,raw=command('cmd-Voice-lib-compile',C+['test',*flags,'-p','lamp-voice','--lib','--no-run','--message-format=json']);rows=cargo_artifacts(raw);save('Voice-compiler-artifacts.json',rows);local_bindings(rows,'Voice-lib');selected=[a for a in rows if a.get('executable') and a['profile']['test']];assert len(selected)==1;a=selected[0];assert a['package_id']==voice['id'] and a['target']['name']=='lamp_voice' and a['target']['kind']==['lib'] and a['profile']['test'] and a['profile']['debuginfo']==0 and a['profile']['debug_assertions'] and a['profile']['overflow_checks'] and a['fresh'] is False
  transitions={}
  for i,helper in enumerate(helpers):
   q=pathlib.Path(helper['executable']);before=helper_phase[str(q)];after=binding(q);normal_alias=helper['target']['kind']==['bin'] and q.parent==T/'debug';keys=set(before)-({'inode'} if normal_alias else set());assert all(before[k]==after[k] for k in keys),str(q);transitions[str(q)]={'before':before,'after':after,'metadata_derived_normal_alias':normal_alias,'only_prospectively_allowed_change':'inode for this unconsumed compiler-phase alias; active guards begin after compile'};freeze(helper,'current-helper-'+str(i))
  save('helper-compiler-phase-transition.json',transitions)
  module=freeze(a,'Voice-lib');save('actual-executables.json',actual);save('new-frozen.json',frozen);save('cache-freshness-observation.json',{'epoch':epoch,'Voice_test_fresh':a['fresh'],'blanket_transitive_freshness_claim':False})
  markers=[x.encode() for x in P['known_forbidden_diagnostic_markers']]
  for name in actual:
   carry=b''
   with pathlib.Path(name).open('rb') as f:
    while chunk:=f.read(1024*1024):
     data=carry+chunk;assert not any(x in data for x in markers),'Known controlled diagnostic marker found';carry=data[-max(map(len,markers)):]
  save('known-diagnostic-marker-check.json',{'known_markers':P['known_forbidden_diagnostic_markers'],'all_current_actuals_checked':list(actual),'none_found':True,'unknown_marker_absence_claim':False})
  dep=module.with_suffix('.d');assert dep.is_file();dr=dep.read_text().replace('\\\n','');first=next(line for line in dr.splitlines() if ': ' in line);inputs=shlex.split(first.split(': ',1)[1]);reached={}
  for name in inputs:
   q=pathlib.Path(name);q=q if q.is_absolute() else W/'lampOS'/q
   if q.resolve().is_relative_to(W):
    rel=q.resolve().relative_to(W).as_posix();assert rel in initial['files'] and sha(q)==initial['files'][rel]['sha256'];reached[rel]=initial['files'][rel]
  assert P['required_cache_epoch_path'] in reached;save('Voice-actual-depinfo.json',{'depinfo_sha256':sha(dep),'actual_local_inputs':reached,'package_prefix_inputs_bound_separately':True})
  inventory={}
  for label,args in [('all',[]),('ignored',['--ignored'])]:
   _,raw=command('cmd-inventory-'+label,[str(module),'--list','--format','terse',*args],P['inventory_timeout_s'],consume=[module]);inventory[label]=[l.removesuffix(': test') for l in raw.splitlines() if l.endswith(': test')];assert len(inventory[label])==len(set(inventory[label]))
  ref=bound(P['reader_source_reference']);assert ref['sha256']==initial['files'][ref['path']]['sha256'];reader=[n for n in inventory['all'] if n.startswith('session_reader::tests::')];assert sorted(reader)==ref['darwin_all'] and not any('::linux::' in n for n in reader) and set(inventory['ignored'])<=set(inventory['all']);assert set(inventory['ignored'])&set(reader)==set(ref['darwin_ignored']);save('inventory.json',{**inventory,'Reader_actual_platform_cases':reader,'Linux_cases_unrun_on_Mac':ref['linux_all']})
  assert initial['files'][P['immutable_stalled_control_source_path']]['sha256']==P['immutable_stalled_control_source_sha256']
  prefixes=P['runtime_module_prefixes'];assert len(prefixes)==len(set(prefixes))==2 and all(prefix.endswith('::') for prefix in prefixes)
  selected={prefix:[n for n in inventory['all'] if n.startswith(prefix)] for prefix in prefixes};assert all(selected.values()) and not set(selected[prefixes[0]])&set(selected[prefixes[1]])
  trace_ref=bound(P['trace_source_reference']);assert trace_ref['sha256']==initial['files'][trace_ref['path']]['sha256'] and sorted(selected['service_trace::tests::'])==trace_ref['darwin_all'];assert all(case in selected['service_trace::tests::'] and case not in inventory['ignored'] for case in P['required_control_cases'])
  save('selected-module-inventory.json',{'modules':selected,'ignored':{prefix:[n for n in inventory['ignored'] if n in values] for prefix,values in selected.items()},'disjoint':True,'full_Voice_runtime_claim':False})
  module_results={};failed_modules=[]
  for prefix in prefixes:
   label='cmd-module-'+prefix.split('::')[0];terminal['runtime_attempts']+=1
   r=None;raw=''
   try:r,raw=command(label,[str(module),prefix,'--test-threads=1'],P['module_runtime_timeout_s'],consume=[module],allowed=(0,101),postguard=False,defer_validation=True)
   finally:
    raw_path=E/(label+'.stdout.txt');raw=raw_path.read_text() if raw_path.exists() else raw
    progress=raw.split('\nfailures:\n',1)[0];raw_rows=re.findall(r'^test (.+?) \.\.\. (ok|FAILED|ignored)(?:,.*)?$',progress,re.M);observed=[(n.removesuffix(' - should panic') if n not in inventory['all'] and n.endswith(' - should panic') and n.removesuffix(' - should panic') in inventory['all'] else n,status) for n,status in raw_rows];annotations=[{'raw_name':rn,'compiled_identity':n,'annotation':' - should panic'} for (rn,rs),(n,status) in zip(raw_rows,observed) if rn!=n];counts=collections.Counter(status for _,status in observed);summary=re.findall(r'^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out',raw,re.M)[-1:]
    result={'module_prefix':prefix,'rows':observed,'raw_rows':raw_rows,'libtest_annotation_mapping':annotations,'counts':dict(counts),'command_exit':r['exit_code'] if r else None,'raw_summary':summary,'saved_before_post_runtime_guard':True,'full_Voice_runtime_claim':False};module_results[prefix]=result;save(label+'-census.json',result)
   guard(phase=label+'-after',consume=[module]);assert r['exit_code']==r['direct_wait_status'] and r['exit_code'] in [0,101] and r['owned_group_absent_after_cleanup'] and not r['unresolved_owned_pids'] and not r['timed_out'] and not r['disk_reserve_breached'] and not r.get('unexpected_owned_descendants');expected=selected[prefix];assert len(observed)==len(set(n for n,_ in observed)) and set(n for n,_ in observed)==set(expected) and {n for n,status in observed if status=='ignored'}==set(inventory['ignored'])&set(expected);assert summary==[(str(counts['ok']),str(counts['FAILED']),str(counts['ignored']),'0',str(len(inventory['all'])-len(expected)))]
   if r['exit_code']!=0 or counts['FAILED']!=0:failed_modules.append(prefix)
  save('observed-module-censuses.json',{'modules':module_results,'runtime_attempts':terminal['runtime_attempts'],'selected_modules_disjoint':True,'no_full_Voice_claim':True,'Linux_Reader_unrun_on_Mac':ref['linux_all']})
  terminal.update(module_counts={prefix:{'passed':result['counts'].get('ok',0),'failed':result['counts'].get('FAILED',0),'ignored':result['counts'].get('ignored',0)} for prefix,result in module_results.items()},actual_full_inventory_count=len(inventory['all']),Reader_platform_cases=reader,Reader_cases_executed_once=True,service_trace_cases_executed_once=True,Voice_test_fresh=False,strict_packages=names,failed_modules=failed_modules,full_Voice_runtime_qualification=False)
  assert terminal['runtime_attempts']==2 and not failed_modules
  terminal['status']='PASS'
 except BaseException as error:terminal['error']=repr(error)
 finally:
  try:guard(full=True,phase='final');save('source-after.json',Q.snapshot(set(P['approved_dirty_paths'])));save('official-source-final.json',official);save('canonical-stat-final.json',source_stats(W,initial,P['branch']));save('official-stat-final.json',source_stats(O,official,P['official_branch']));save('protected-after.json',{**protected,**frozen});save('actual-after.json',{n:{'bound':b,'observed':binding(n)} for n,b in actual.items()});terminal.update(source_unchanged=True,official_owner_unchanged=True,sealed_and_actual_guards=True,protected_count=len(protected)+len(frozen),actual_count=len(actual),full_hash_checkpoints=hash_points)
  except BaseException as error:terminal.update(status='FAIL',guard_error=repr(error))
  if S.OWNED_OPEN:terminal.update(status='FAIL',cleanup_error='Owned groups unresolved; target lease held')
  save('consumed-SHA-checkpoints.json',consumed);terminal.update(owned_groups_closed=not S.OWNED_OPEN,unresolved_owned_pids=sorted(S.OWNED_OPEN),lease_released=not S.OWNED_OPEN);save('terminal.json',terminal)
 return 0 if terminal['status']=='PASS' else 1
if __name__=='__main__':raise SystemExit(main())
