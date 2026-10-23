
use strict; use warnings; use Time::HiRes qw(clock_gettime); use JSON::PP qw(encode_json);
$|=1;
my $wanted=shift @ARGV;
sub snap { my($phase)=@_; my $ns=readlink('/proc/self/ns/time');
  open(my $fh,'<','/proc/self/timens_offsets') or die $!; local $/; my $off=<$fh>;
  return {phase=>$phase,pid=>$$,uid=>$<,monotonic=>clock_gettime(1),boottime=>clock_gettime(7),
          realtime=>clock_gettime(0),time_namespace=>$ns,offsets=>$off}; }
my $before=snap('namespace_before'); print 'BOOT_CLOCK_RECEIPT '.encode_json($before)."\n";
die 'namespace clock not at requested boundary' unless $before->{monotonic}>=$wanted &&
 $before->{monotonic}<$wanted+2 && $before->{boottime}>=$wanted && $before->{boottime}<$wanted+2;
my $pid=fork(); defined($pid) or die $!;
if(!$pid) { print 'BOOT_CLOCK_RECEIPT '.encode_json(snap('test_child_before_exec'))."\n";
  exec {$ARGV[0]} @ARGV; die 'exec failed: '.$!; }
waitpid($pid,0); my $status=$?;
my $after=snap('namespace_after'); $after->{test_wait_status}=$status;
print 'BOOT_CLOCK_RECEIPT '.encode_json($after)."\n";
exit(($status & 127) ? 128+($status & 127) : ($status >> 8));
