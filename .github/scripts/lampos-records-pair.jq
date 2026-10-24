# CI-only schemas, used by the shell driver and its offline negative controls.
def must($ok; $message): if $ok then . else error($message) end;
def tests: [
  "records_6408_prunes_6449_private_sessions_and_preserves_live_invocation",
  "records_6449_prunes_6408_private_sessions_and_preserves_live_invocation",
  "records_6408_refuses_6449_shared_layout_and_6449_can_resume"
];
def pins: {
  "6408": {
    source: "ee53765c8d014eb00e41c1a5a7054d2546c18bdb",
    "lamp-records": {asset:668607851,size:1393272,sha256:"a1839349ca730d0051033bf37a43238dfc0deb2a32184e600893424724722031"},
    "lamp-voice": {asset:668607984,size:28375944,sha256:"af7cd0d1e384ebddcca2b58eea94176c80aba573a5f47f4cd95cc8ce1a7ad084"},
    "lampos-manifest.json": {asset:668607996,size:13563,sha256:"696c6ae70a5cdf164c4a319bdc0887e5d1286519c36157a9c0c8db2cc0d80734"}
  },
  "6449": {
    source: "be16c3327524ccef8a1908a3b8ebf40c16efba8a",
    "lamp-records": {asset:669530983,size:1406976,sha256:"b18095bc74064e9949d9ed16c8f3c9a4c8c46e35892d3922800cf4aea8bb668f"},
    "lamp-voice": {asset:669531071,size:28372464,sha256:"21783ba5fd6654d96164f7426dc27f2c72f06a074ce91fe06b256feec697c6da"},
    "lampos-manifest.json": {asset:669531073,size:13563,sha256:"2f8f218c41b74576b52451248f0df2f9251634bfc68658b42062282bc0c68a6a"}
  }
};
def expected_artifacts($root): [pins | to_entries[] | .key as $release | .value as $pin
  | ["lamp-records","lamp-voice"][] as $part
  | {release:($release|tonumber),part:$part,source:$pin.source,sha256:$pin[$part].sha256,
     size:$pin[$part].size,path:($root+"/"+$release+"/"+$part)}];
def terminal($artifact_root): . as $receipts | tests | map(. as $name | $receipts[$name+".json"]
  | must(type=="object"; "missing terminal case "+$name)
  | must(.version==1 and .test==$name and .result=="passed"; "wrong terminal identity "+$name)
  | must(.expected_pair==[6408,6449] and .architecture=="aarch64"
      and .native_root_systemd==true and .no_guard_override==true; "wrong native pair/guard "+$name)
  | must(has("error") and .error==null and has("cleanup_error") and .cleanup_error==null
      and .cleanup_confirmed==true; "case failure/internal cleanup unconfirmed "+$name)
  | must(.signature_trust_verified==false and .full_rotation_or_device_acceptance==false;
      "unsupported acceptance claim "+$name)
  | must((.artifacts|type)=="array" and (.artifacts|length)==4; "wrong artifact inventory "+$name)
  | must((.artifacts|sort_by(.release,.part))==(expected_artifacts($artifact_root)|sort_by(.release,.part));
      "wrong artifact source/path/hash/size "+$name)
  | must((.observations|type)=="array" and (.observations|length)>0 and (.observations|length)<=64;
      "missing bounded observations "+$name)
  | must((.commands|type)=="array" and (.commands|length)>0 and (.commands|length)<=128;
      "missing bounded command evidence "+$name)
  | must(all(.commands[]; type=="object" and .group_absent!=false and .reaped!=false
      and .unconfirmed_command_cleanup!=true); "unconfirmed command cleanup "+$name)
  | $name);
def ledger: must(type=="object" and keys==["units","version"] and .version==1; "invalid ownership ledger")
  | must((.units|type)=="array" and (.units|length)<=24
      and (.units|unique|length)==(.units|length); "unbounded/duplicate owned units")
  | must(all(.units[]; type=="string" and test("^lamp-records-pair-[0-9]+-[a-z0-9-]+-[0-9]+\\.service$"));
      "unrelated unit in ledger");
def unit_fields($unit): must(type=="object" and
    keys==["ActiveState","ControlGroup","ControlPID","Id","Job","LoadState","MainPID"]
    and .Id==$unit; "missing/wrong owned unit fields")
  | must(.ControlGroup=="" or .ControlGroup==("/system.slice/"+$unit); "unexpected owned cgroup");
def inactive: .MainPID=="0" and .ControlPID=="0" and (.Job=="" or .Job=="0")
  and (.LoadState=="not-found" or .ActiveState=="inactive" or .ActiveState=="failed");
def empty_v2_group: must(.controllers==true; "unified cgroup v2 required")
  | if .absent==true then true else
      must(.members=="" and (.events|type)=="string"; "live members or missing v2 group events")
      | (.events|split("\n")|map(select(startswith("populated "))))==["populated 0"]
    end;
def manifest($release): pins[$release] as $pin
  | must(.schema==1 and .channel=="lamp-voice" and .sequence==($release|tonumber)
      and .commit==$pin.source and .files_url==("https://github.com/autonomous-ai/lampOS/releases/download/lampos-lamp-voice-"+$release+"/");
      "decoded manifest source/release mismatch")
  | must((.files|type)=="array" and (.files|length)<128; "unbounded manifest inventory")
  | must(([.files[]|select(.name=="lamp-records")])==[{name:"lamp-records",sha256:$pin["lamp-records"].sha256,size:$pin["lamp-records"].size}]
      and ([.files[]|select(.name=="lamp-voice")])==[{name:"lamp-voice",sha256:$pin["lamp-voice"].sha256,size:$pin["lamp-voice"].size}];
      "manifest payload mismatch/duplicate");
def selected($root): . as $rows
  | must(type=="array" and ([$rows[]|select(.reason=="build-finished")|.success])==[true]
      and .[-1].reason=="build-finished"; "incomplete Cargo output")
  | [$rows[]|select(.reason=="compiler-artifact" and .target.name=="release_pair_systemd")]
  | must(length==1; "ambiguous/missing current test executable") | .[0]
  | must(.target.kind==["test"] and .profile.test==true
      and .target.src_path==($root+"/lampOS/crates/records/tests/release_pair_systemd.rs")
      and (.executable|type)=="string"; "wrong test source/profile")
  | .executable as $exe
  | must(($exe|startswith($root+"/lampOS/target/debug/deps/release_pair_systemd-"))
      and ($exe|ltrimstr($root+"/lampOS/target/debug/deps/")|test("^release_pair_systemd-[a-f0-9]+$"))
      and (.filenames|index($exe))!=null; "wrong Cargo executable path") | $exe;
