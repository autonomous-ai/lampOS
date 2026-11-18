# Body sounds: measuring the bars (#7685)

These are the steps for measuring the bars of `body_sounds::BARS` on
lamp-06c1, with El's consenting person. They use `lamp-ears-body-kit`
([ears.md](ears.md), "Body sounds"). They need a build with #7696's
`LAMP_EARS_BODY_SOUNDS` and this kit.

## The rules (El, 12:51 on #7685)

- The recordings stay on the lamp, in a private folder. Here that is
  `/root/body-kit`: 0700, files 0600, root only.
- The folder is deleted once the bars are set and confirmed on #7685
  (step 7).
- Only the person's own sounds are recorded. Each clip is 3 s, taken only
  when the operator presses Enter. Nobody else talks during a take. The
  speech and laughter takes are the person's own.
- The office hour logs scores only (step 5). No sound is kept, and it runs
  only while the people there know it is on.

## 0. Build and copy (on the Mac, from `lampOS/`)

```sh
cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.36 -p lamp-ears --bin lamp-ears-body-kit
scp target/aarch64-unknown-linux-gnu/release/lamp-ears-body-kit pi@lamp-06c1:/tmp/
```

The lamp's `lamp-ears` must be #7696's or later (`ears/classes` with the body
scores). Check with `sudo systemctl cat lamp-ears | grep LAMP_EARS_CLASSES`:
it must say `on`.

## 1. Open the microphone's blocks to the kit, for the takes only

Drop-ins under `/run` are gone at the next boot.

```sh
sudo mkdir -p /run/systemd/system/lamp-ears.service.d
printf '[Service]\nEnvironment=LAMP_BUS_AUDIO=on LAMP_EARS_BODY_SOUNDS=on\n' \
  | sudo tee /run/systemd/system/lamp-ears.service.d/body-kit.conf
sudo systemctl daemon-reload && sudo systemctl restart lamp-ears
```

## 2. The person's sounds: 20 of each

Ask the person "ready?" first. For each label, the kit says
`take 1 of 20`. Press Enter. At `now`, the person makes the sound once, at
the lamp, as they would at the desk. It prints the clip's peak scores.
Type `r` and Enter to take the last one again, or `q` and Enter to stop.

```sh
sudo /tmp/lamp-ears-body-kit take /root/body-kit cough
sudo /tmp/lamp-ears-body-kit take /root/body-kit sneeze
sudo /tmp/lamp-ears-body-kit take /root/body-kit throat_clear
sudo /tmp/lamp-ears-body-kit take /root/body-kit sniff
```

A real sneeze can't be made at will. Take the ones that come over the
afternoon: `take` goes on from the last number, so `--count 1` takes one
more.

## 3. The 60 other sounds: 10 of each

```sh
sudo /tmp/lamp-ears-body-kit take /root/body-kit speech --count 10      # the person talking
sudo /tmp/lamp-ears-body-kit take /root/body-kit laughter --count 10    # the person laughing
sudo /tmp/lamp-ears-body-kit take /root/body-kit lamp_voice --count 10  # ask the lamp something, Enter as it answers
sudo /tmp/lamp-ears-body-kit take /root/body-kit typing --count 10
sudo /tmp/lamp-ears-body-kit take /root/body-kit thump --count 10       # a hand on the desk
sudo /tmp/lamp-ears-body-kit take /root/body-kit chair --count 10       # a chair moved or sat in
```

## 4. Close the microphone's blocks again; keep the body scores on

```sh
printf '[Service]\nEnvironment=LAMP_EARS_BODY_SOUNDS=on\n' \
  | sudo tee /run/systemd/system/lamp-ears.service.d/body-kit.conf
sudo systemctl daemon-reload && sudo systemctl restart lamp-ears
```

## 5. The office hour: scores only

Tell the people in the room that it is on. Then:

```sh
sudo /tmp/lamp-ears-body-kit office /root/body-kit --minutes 60
```

It writes `/root/body-kit/office.csv`: the end time of each window, whether
the lamp spoke into it, and the five body scores. Nothing else, and no
sound. Ctrl-C stops it early; what was logged stays usable.

## 6. The bars

```sh
sudo /tmp/lamp-ears-body-kit suggest /root/body-kit
```

For each class it gives the lowest bar, in steps of 0.01, over every other
sound's peak, so none of the 60 others is a false hit. It also prints:

- the recall there, as the person's clips over the bar;
- the precision: the share of all clips over the bar that are the class's
  own (a sneeze over the cough bar counts against it);
- the refractory;
- the alarms an hour that the lamp's own detector would give over the
  office log.

A class is `publish yes` only with recall of at least 0.9 and under one
alarm an hour (ears.md). It then prints `BARS` as it would read. A class
that fails stays `None`. Post the table on #7685. The bars go into
`body_sounds::BARS` in a pull request, with the table in its body.

## 7. Delete the folder, and the drop-in

Do this once the bars are confirmed on #7685:

```sh
sudo /tmp/lamp-ears-body-kit delete /root/body-kit
sudo rm /run/systemd/system/lamp-ears.service.d/body-kit.conf
sudo systemctl daemon-reload && sudo systemctl restart lamp-ears
rm /tmp/lamp-ears-body-kit
```

`delete` zero-fills each clip and removes the kit's files (`LABEL-NN.wav`,
`LABEL-NN.csv`, `office.csv`) and then the folder. It removes nothing else.
If anything else is in the folder, it names it and leaves the folder. A
flash card may still hold old blocks after a file is removed, which is why
the folder stays root's and private until it is gone. Say on #7685 that it
is deleted.
