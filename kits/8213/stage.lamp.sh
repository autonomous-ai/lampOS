# lamp, root sh. Installs the two signed local channels the Mac copied to /tmp/t8156 into $CH, and records the
# "before" read. Changes nothing of lampOS's own: no unit, record, key, link or setting. Prints STAGED OK.
cd /
(cd $IN && sha256sum -c --quiet SHAS) || { echo "STAGE STOP: copied files do not match SHAS"; exit 3; }
[ ! -e $CH ] || { echo "STAGE STOP: $CH exists"; exit 3; }
mkdir -p $CH; chmod 755 $CH
cp -a $IN/ch1 $IN/ch2 $CH/; install -m 0644 $IN/test.pub $CH/test.pub
chown -R root:root $CH; find $CH -type d -exec chmod 755 {} +; find $CH -type f -exec chmod 644 {} +
# Binaries keep their execute bit (a round copies them into the new build folder and checks sha256 only).
for f in $CH/ch1/lamp-*; do case "$f" in *.service|*.timer|*.socket|*.path) ;; *) chmod 755 "$f" ;; esac; done
(cd $CH && sha256sum -c --quiet $IN/SHAS) || { echo "STAGE STOP: installed channel differs"; rm -rf $CH; exit 3; }
mkdir -p $R/before; tar -C $REC --exclude=./lock -cpf $R/before/rec.tar .   # evidence only; never written back
build > $R/before/state.txt; ls $UPD > $R/before/updfolders.txt
n "staged local channels in $CH (ch1 = the PR build, ch2 = stable's bytes at a higher sequence), test key $CH/test.pub"
rm -rf $IN
echo "STAGED OK"
