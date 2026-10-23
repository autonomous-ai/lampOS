
use strict; use warnings; use Time::HiRes qw(clock_gettime); use JSON::PP qw(encode_json);
$|=1;
my $wanted=shift @ARGV; my $inner=shift @ARGV;
sub snap { my($phase)=@_; my $ns=readlink('/proc/self/ns/time');
  open(my $fh,'<','/proc/self/timens_offsets') or die $!; local $/; my $off=<$fh>;
  return {phase=>$phase,pid=>$$,uid=>$<,monotonic=>clock_gettime(1),boottime=>clock_gettime(7),
          realtime=>clock_gettime(0),time_namespace=>$ns,offsets=>$off}; }
my $before=snap('outer_before'); print 'BOOT_CLOCK_RECEIPT '.encode_json($before)."\n";
my $om=$wanted-int($before->{monotonic}); my $ob=$wanted-int($before->{boottime});
my $pid=fork(); defined($pid) or die $!;
if(!$pid) { exec 'unshare','--user','--map-current-user','--time','--fork',
  '--monotonic='.$om,'--boottime='.$ob,'perl','-e',$inner,'--',$wanted,@ARGV; die $!; }
waitpid($pid,0); my $status=$?;
my $after=snap('outer_after'); $after->{namespace_wait_status}=$status;
print 'BOOT_CLOCK_RECEIPT '.encode_json($after)."\n";
exit(($status & 127) ? 128+($status & 127) : ($status >> 8));
