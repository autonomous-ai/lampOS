# lamp, root sh (common first). Channel mode only: the two signed channels the runner made, copied by the Mac to
# /tmp/t8337, installed into $CH (readable in lamp-update's sandbox). Changes nothing of lampOS's own. Prints STAGED OK.
cd /
(cd $IN && sha256sum -c --quiet SHAS) || { echo "STAGE STOP: copied files do not match SHAS"; exit 3; }
[ ! -e $CH ] || { echo "STAGE STOP: $CH exists (an earlier run not put back)"; exit 3; }
mkdir -p $CH; chmod 755 $CH
cp -a $IN/ch1 $IN/ch2 $CH/; install -m 0644 $IN/test.pub $CH/test.pub
chown -R root:root $CH; find $CH -type d -exec chmod 755 {} +; find $CH -type f -exec chmod 644 {} +
# Binaries keep their execute bit (a round copies them into the new build folder and checks sha256 only).
for f in $CH/ch1/lamp-*; do case "$f" in *.service|*.timer|*.socket|*.path) ;; *) chmod 755 "$f" ;; esac; done
(cd $CH && sha256sum -c --quiet $IN/SHAS) || { echo "STAGE STOP: installed channel differs"; rm -rf $CH; exit 3; }
n "staged the runner's channels in $CH (ch1 = the PR build, ch2 = stable's bytes at a higher sequence)"
rm -rf $IN
echo "STAGED OK"
