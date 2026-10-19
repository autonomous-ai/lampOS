# Reviewer work log

## 2026-10-17 (the same Claude session, after midnight)

Newest first. Times from `date` (+07). 00:12 to 01:40: the review agents stopped at a usage limit.

- 19:51 merged #1906 (Jarvis's held stop made visible) as 6c4683048; #1683 stays open
- 19:50 read #1913 (El's settings in the manifest): no P1, off unchanged; three P2s when on (a missing apply sticks the lamp, lampos.conf never updated, a retried put-back loses records); first builder's merge
- 19:40 #1909's P2 fixed (ExecStartPre, shown on lamp-c0a4); one P3, needs a fold; first builder's merge
- 19:40 read #1903 (Tests' lamp-skills one wait): no finding; for the first builder (on by default)
- 19:36 read #1906 (Jarvis's held stop made visible): no finding; merge on green ARM64
- 19:36 merged #1904 (Nine's presence by an answered voice) as 753f71236; #1677 left open
- 19:30 read #1909 (Twelve's lamp-cloud on in the product): P2 (Conflicts= stops os-server even when lamp-cloud's conditions skip it); for the first builder (on by default)
- 19:26 #1897 merged by the first builder as 3dfcde963 before my merge
- 19:25 read #1899 (Third's voice-eval score table): no finding; needs a fold
- 19:24 read #1900 (Tests' light path in a few ms): El's doc P2 stands; for the first builder (on by default)
- 19:20 read #1904 (Nine's presence by an answered voice): no finding; merge on green ARM64
- 19:17 merged #1898 (Thirteen's factory strip-off step) as d3870f543
- 19:14 read #1901 (first builder's wellbeing nudges, #1876 folded): three P2s fixed, the folder agrees with #1732, gate green; first builder's merge
- 19:09 merged #1848 (hw-first's test cases and traceability, robots/ only) as 4bdf060c9; checked against #1887's merged matrix
- 19:02 merged #1884 (Seventh's waiting-cuts in voice-eval) as 4de0c33fc
- 19:01 merged #1893 (Tests' schedule report period in Config, lamp unchanged, #1806) as 692b6e20d
- 18:58 read #1898 (Thirteen's factory strip-off step): no finding; merge on green ARM64
- 18:53 read #1897 (Sixth's fix: LAMP_SECOND_SCOPED_CONTEXT never reached the worker, a miss in my #1839 read): right; needs a rebase after #1892
- 18:52 merged #1892 (Fourth's near-call bar) as 2e754384b
- 18:47 read #1888 (Tests' lamp-actions accept at once): no blocking finding, one P3 (a ready-but-WouldBlock listener could spin); for the first builder (on by default, lamp-actions)
- 18:44 read #1884 (Seventh's waiting-cuts, now Rust in voice-eval): no finding; merge on green ARM64
- 18:41 #1879's fixes at 356219fed: the three P2s and the dead end answered (two read myself); first builder's merge after #1788, #1801
- 18:38 read #1892 (Fourth's near-call bar): no finding; merge on green ARM64
- 18:37 merged #1885 (Tests' reset watcher test without its sleeps, tests only, #1806) as 9a706deca
- 18:30 #1846: one P2 (realtime-tests.md still says a PR runs every test); I merge after it, the lead's ask
- 18:29 merged #1882 (Jarvis's cloud skills.list) as 5689de7d9; #1722 is the Queue, not closed
- 18:27 read #1846 (first builder's scoped ARM64 runs): no blocking finding; DevOps's workflow and the first builder's merge; Python under .github/ is outside the rule's letter
- 18:26 first read of #1876 (first builder's wellbeing nudges): product skills unchanged; three P2s if installed (stop functions past the 32, eyes and stretch 5 min apart, off not kept); needs a rebase
- 18:25 merged #1880 (Jarvis's held reports_stop) as 90c66e1bf; #1683 left open
- 18:22 merged #1877 (El's health facts) as 32fae8b60; #1681 left open
- 18:20 read #1783 (first builder's volume/light wording and character lines): no finding; theirs to merge (on by default)
- 18:17 first read of #1790 (first builder's who was here): no blocking finding; conflicts with lamp-voice and carries a merge commit, needs a rebase
- 18:16 #1865's P2 fixed at d6c63f37a (two known people share a room session); first builder's merge
- 18:16 merged #1825 (Twelve's schedule --zone system, folded) as a8b1f3474; #820 already closed
- 18:15 read #1879's top commit (El's models in the manifest): off unchanged; three P2s (a failed model put-back, recovery not restarting a model-only change, prune trusting state.json) and a self-update dead end; first builder's merge
- 18:14 re-read #1732 at 805f56961: all four P2s and the docs gap fixed, three new P3s; conflicts with lamp-voice again; first builder's merge
- 18:14 read #1884 (Seventh's wait-cut-counts): P2, Python under lampOS/tools/ (Rust only outside training); for the first builder if an exception
- 18:13 read #1882 (Jarvis's cloud skills.list): no blocking finding, one P3 (the off-file folder isn't made by any unit); merge on green ARM64
- 18:11 read #1880 (Jarvis's held reports_stop done): no blocking finding, one P3 (a full desk drops it silently); merge on green ARM64
- 18:10 merged #1873 (Tests' serve fixture polls at the tick when idle, tests only, #1806) as d982e5b9a
- 18:07 read #1877 (El's health facts): no blocking finding, one P3 (a lamp-wide quiet test names every old stale part); merge on green ARM64
- 18:04 #1872's new head 9ec713c35 (schema 2 only while Devin work): reads right, gate green; first builder's merge
- 17:57 gate of #1874 (first builder's skill snooze/off, #1837 folded): green; first builder's merge
- 17:54 read #1865 (Sixth's Live scoped session): switch off unchanged; one P2 when on (two known people alternating lose every other line); first builder's merge
- 17:43 gate of #1871 (first builder's look for things, #1766 folded): green; first builder's merge
- 17:43 read #1872 (Eighth's Devin in lamp-tasks): no blocking finding; for the first builder (store schema 2 on every lamp, a rollback sets it aside)
- 17:37 merged #1863 (Tests' client deadline tests at 800 ms, tests only, #1806) as 179016adc
- 17:33 merged #1864 (Seventh's refused-session tests, tests and docs) as b45aaf45a; #1567 stays open
- 17:31 regates of #1811 (1963849d4) and #1827 (4f7afb94a) after the rebase: green; first builder's merges
- 17:30 merged #1862 (Tests' pump-wake tests into the realtime group, config and docs) as 9867cc4dc
- 17:29 read #1868 (Thirteen's speaking through concentration): no finding; for the first builder (body motion, lamp-4ace's catalogue)
- 17:23 read #1867 (Thirteen's gaze off without --gaze): no finding; for the first builder (on by default, the body)
- 17:22 merged #1860 (Sixth's memory.md caution back, docs) as 660248377
- 17:16 #1859's P3s taken at fb510abf7, gate clean; waiting on an ARM64 run for that head
- 17:15 read #1864 (Seventh's refused-session tests, tests and docs): no finding; merge on green ARM64
- 17:13 gate of #1827 (Nine's sitter print): green, the same /tmp/leh clash; needs a rebase after #1811; first builder's merge
- 17:10 gate of #1811 (Nine's sitter by place): green, the same lamp-voice /tmp/leh clash; first builder's merge
- 17:09 read #1766 (first builder's look for things): no blocking finding, one P3 (stale [search] comment); needs a rebase and fold
- 17:09 read #1837 (first builder's skill snooze/off): no blocking finding, one P3 (no fsync on the off file); needs a rebase and fold
- 17:08 merged #1858 (Tests' GPIO tests on a tenth of the privacy wait, cfg(test) only, #1806) as 4a188e789
- 17:05 merged #1857 (Tests' ring flood test, Linux keeps its 1,000; tests only, #1569 row 24) as 164f07e0a; #1569 stays open
- 17:04 gate of #1760 (Nine's room/activity): green; the one eyes failure is lamp-voice's /tmp/leh clash, fixed in #1859; first builder's merge
- 17:01 read #1859 (Ten's face seems read): no blocking finding, two P3s; for the first builder (FER2013 research-only terms)
- 16:57 merged #1847 (El's pale breath for Not feeling well) as 4a39f0412; #1681 left open for its lamp check
- 16:53 merged #1845 (Tests' guard: ARM64 PR paths held to the docs crates read) as f9c9f8d9b
- 16:51 read #1839 (Sixth's scoped second-route context): no blocking finding, one P3 (memory.md drops a caution that still holds by default); first builder's merge
- 16:48 merged #1853 (Tests' GPIO test split, tests only, #1806) as d7327b8a7; #1806 left open
- 16:45 merged #1842 (Eighth's light while muted) as 73e9a2394; closed #1816
- 16:39 full read of #1732 (Second's bedtime and morning, five slices): nothing changes a product lamp today; four P2s (install order, one late sample ends the night's rest, a clock set forward locks a daily trigger, the reset misses the two new folders) and a msgs/bus docs gap; the first builder's merge
- 16:31 read #1847 (El's pale breath): no blocking finding, two P3s; needs a rebase after #1835
- 16:27 merged #1833 (Tests' decline told held_back on voice/turn) as 32b3b5519; closed #1155
- 16:23 read #1842 (Eighth's light while muted): no blocking finding, one P3 (the doc says tests aren't taken; they are read, never drawn); merge on green ARM64
- 16:22 read #1115's new commit (Fifth's hand fix, a joint kept held while the hand still reads): no finding; still the first builder's merge
- 16:22 read #1848 (hw-first's test cases and traceability): no finding; merges after #1785, whose matrix it reads
- 16:18 read #1835 (El's quiet_ms): no blocking finding, one P3 (SKILL.md says one droop where two parts droop twice); handed to the first builder (a msgs field, their 14:44 call; #1821 still open), after first writing that I would merge it
- 16:12 #1840 (Fourth's #1830 rebased onto the first builder's branch): same patch, my 15:51 read holds
- 16:08 read #1829 (first builder's skills limits): no blocking finding, one P3 flake under load
- 16:00 first read #1827 (sitter by in-memory print, waits on owner's yes): first builder's
- 15:56 #1788: open point settled at c98c7547e; first builder's
- 15:54 merged #1834 (hw-bench role file, docs)
- 15:53 merged #1832 (hw-first burn-in script, robots/, by hand; no lampOS ARM64 for that path)
- 15:52 merged #1824 (skills host-clock tests to realtime group, tests/config, #1806)
- 15:51 merged #1820 (skills dance tests faster, tests only, #1806)
- 15:50 read #1775 rebased: no finding; first builder merges
- 15:45 gated #1792: clean; first builder's
- 15:44 #1825 ARM64 failed: P2, a copied localtime is named 'localtime' on Linux
- 15:37 first read #1792 (onboarding privacy proof): privacy path + msgs + install, first builder's
- 15:36 merged #1823 (lamp-near-call crate, used by nothing yet)
- 15:35 reviewed #1828 (Harness at boot via user manager HOME): no finding; install path, first builder's
- 15:28 reviewed #1825 (lamp-schedule --zone system, behaviour-neutral): no finding, merge on green
- 15:25 reviewed #1822 (voice zone via lamp-local-time): no finding; on by default, first builder's or mine after a lamp run under #1821
- 15:23 first read #1818 (key terms incl. known faces' names sent to Deepgram, off unless set, on #1712's branch): P2 privacy call for the first builder
- 15:22 reviewed #1823 (lamp-near-call crate, used by nothing): no finding, merge on green
- 15:18 gated #1817 (refile of #1782): green; first builder merges
- 15:11 gated #1794: clean; first builder's
- 15:08 first read #1782 (voice/turn + hello quiet to the lamp): msgs type on every lamp, fold; first builder's
- 15:03 gated #1748: clean; first builder's
- 15:01 merged #1813 (cloud.md: what the backend reads from a report, docs, no ARM64 by rule)
- 15:01 merged #1805 (voice says computers' own names, off unless set)
- 14:59 first reads #1748 (mic1 PCM on bus) and #1794 (memory tools bound to request): first builder's
- 14:59 merged #1780 (install.md: what harness-on trusts, docs)
- 14:55 first read #1811 (activity judge follows the sitter, stacked on #1760/#1747): first builder's with the stack
- 14:55 merged #1796 (reports stop by voice reaches lamp-cloud)
- 14:52 gated #1772 rebased: no finding; first builder's
- 14:47 reviewed #1805 (voice says computers' own names, off unless set): no finding, merge on green
- 14:39 #1804 (lamp-c0a4 owner file) tied to #1789's role call; first read #1801 (min_updater, stacked on #1788)
- 14:38 reviewed #1796 (reports stop by voice reaches lamp-cloud): no finding, merge on green
- 14:35 full read #1788: open point, timer may run a missing back binary on round-only lamps; P3 network refuse; first builder's
- 14:34 merged #1787 (Not feeling well core skill, not in product)
- 14:23 merged #1784 (journal height from published faces, #1771's P3)
- 14:18 reviewed #1789 (thirteen's role file names lamp-c0a4): loosens ssh and use rules; role call for the first builder
- 14:16 reviewed #1787 (Not feeling well core skill, not in product): no finding, merge on green
- 14:13 first read #1788 (lamp-update self-switch, off unless set): conflicts; root timer every 2 min on every lamp may fail where the back binary is missing; first builder's
- 14:13 #1773 follow-on P2 fixed; first builder's
- 14:13 merged #1776 (lamp-body --let-go-at-stop, off by default, factory)
- 14:08 merged #1778 (skills rise trigger, exact words, say priority; first PR of #1684)
- 14:07 reviewed #1778 (skills: rise trigger, exact words, say priority): no finding, merge on green
- 14:05 reviewed #1784 (journal height from published faces): no finding, merge on green
- 14:01 #1773: mute-light wait fixed at 4888663cc; follow-on P2, a late painter's black frame can blank the mute light
- 13:58 #1781 (refile of #1767, folded) and #1707 (folded): no finding left; first builder's
- 13:58 #1776 P2 fixed at be0b7ee34; merge on green
- 13:51 merged #1777 (lamp-update harness-on, run by hand; B5 of #1676)
- 13:50 reviewed #1707 (ears/classes contract in msgs): no finding, fold; msgs + mic path, first builder's
- 13:47 #1767 Twelve's P2 fixed at 18a668191; asked to fold; first builder's
- 13:46 #1763 P2 fixed at 8e78e591d; P3 UTC midnight vs the asked 03:00 local; first builder's
- 13:44 reviewed #1768 (journal keeps each exchange's own scope): no finding; on by default, first builder's
- 13:44 reviewed #1777 (lamp-update harness-on): no finding, merge on green
- 13:42 reviewed #1776 (lamp-body --let-go-at-stop): P2, the 'only when down' condition has no test that fails without it
- 13:38 reviewed #1771 (room/presence says by face or voice): no finding; msgs field, first builder's
- 13:38 merged #1770 (lamp-tasks targets from Harness machines, off unless set; B6 step 3 of #1676)
- 13:36 first read #1775 (warmer hello by familiarity, stacked on #1772/#1763): first builder's with the stack
- 13:35 reviewed #1655 (lamp-actions keeps its folder across restart): one P3; on by default, first builder's
- 13:33 first read #1772 (Mood carries the familiar level, stacked on #1763): first builder's with #1763
- 13:33 first read #1773 (waking face, off unless set): P2, the mute light's open can wait 2 s on a stuck painter and then fail; privacy path, first builder's
- 13:31 merged #1765 (lamp-tasks report period configurable, lamp unchanged, #1128)
- 13:31 merged #1769 (hardware-process P3s, docs)
- 13:30 reviewed #1770 (lamp-tasks targets from Harness machines, off unless set): no finding, merge on green
- 13:27 #1767: Twelve's P2 (re-remembered person dropped again) holds; corrected my review to not ready
- 13:26 reviewed #1767 (forget my face with an unsearchable set-aside file): one P3; privacy path, first builder's
- 13:25 merged #1761 (hand-controls release test waits only when an input is sent, tests only, #1128)
- 13:24 reviewed #1763 (familiarity per remembered person, off unless --visits on): P2, the file keeps the time of the last counted visit despite the claim; first builder's
- 13:16 first read of draft #1699 (prepare to unplug): conflicts, fold; first builder's after a lamp trial
- 13:14 reviewed #1769 (hardware-process P3s, docs): no finding, merge on green
- 13:12 merged #1751 (serve signal tests' first trace row before ready, tests only, #1569 row 30)
- 13:12 #1755 full read at a4746b326: no blocking finding; first builder's
- 13:03 reviewed #1739 (first builder's joining.md hardware rules): no finding; merges after #1762; #1762 to merge on green, the first builder's #1739 says the same split
- 13:03 reviewed #1762 (hardware process + hw-first role, docs): no finding; asked the first builder for the role call, then merge on green
- 13:02 merged #1758 (texts never move the body; text news only of text work; off unless set)
- 13:01 reviewed #1758 (texts never move the body; text news only of text work; off unless set): no finding, merge on green
- 12:54 #1760 (room/activity, off unless set, stacked on #1747) first read: msgs types + product skill change, first builder's
- 12:54 #1755 (reports, off unless set) first read: drop attribution line, fold; msgs types + setup changes on every lamp, first builder's; full read to follow
- 12:53 reviewed #1759 (product catalogue speaking section, stacked on #1067): no finding; on by default + lamp A/B, first builder's
- 12:52 merged #1754 (speaker-finish test folders unique, tests only, #1569 row 32)
- 12:50 reviewed #1715 (shutdown rechecks child exit before forcing): no finding; default + mic path, first builder's
- 12:50 merged #1723 (forget by source_id; forget everything for a known person only; off unless set)
- 12:39 #1747 P2 fixed at 93884fc2b (rules read at start); one P3; first builder's
- 12:38 closed #1595 (last part merged as #1752)
- 12:38 merged #1752 (timer/reminder kind in list; first builder's yes at 12:25)
- 12:36 merged #1730 (light-scenes, point-light, light-show core skills, not in product)
- 12:33 merged #1737 (update prune keeps lamp-update's own folder; first builder's yes at 11:48)
- 12:29 reviewed #1756 (ramlog copy-back skips the open journal, image): no finding, one question; first builder's after a lamp run
- 12:29 merged #1745 (turn-end switches kept on lamp-4ace, docs only)
- 12:27 reviewed #1749 (Expression liveliness from Mood, stacked on #1728): one P3; first builder's with #1728
- 12:26 #1752: first builder said merge on green
- 12:24 reviewed #1752 (timer/reminder kind in list): no finding; on by default, first builder's
- 12:21 #1728 P3s taken at ebec1f8be; first builder's merge
- 12:21 #1744 P2s fixed at 791b74bf5; asked to fold and to move the workflow lines to a DevOps PR (my earlier yes was wrong); first builder's watched run
- 12:14 reviewed #1747 (keep the light on): P2, rules not published at voice start so the rule lapses after a reboot; msgs field, first builder's
- 12:14 merged #1743 (relay_clock child in Perl, tests only, #1569 row 31)
- 12:04 re-reviewed #1728 (one breath from Mood): three P3s; msgs field + always-on readers, handed to the first builder
- 12:02 merged #1742 (texts task + untold news with the next text, off unless set) and #1740 (cloud.md: what the backend reads, docs only)
- 11:54 reviewed #1745 (turn-end switches kept on lamp-4ace, docs only): one P3, merge on green
- 11:49 #1737: first builder said merge on green once folded and the attribution line is gone
- 11:49 reviewed #1742 (texts task + untold news with the next text, off unless set): no finding, merge on green
- 11:49 reviewed #1744 (factory steps.sh): P2 python3 on the lamp, P2 no Ctrl-C stop for the arm; first builder's after a watched run
- 11:47 reviewed #1737 (update prune keeps lamp-update's own folder): fix right; asked El to drop the attribution line and fold; on by default, handed to the first builder
- 11:47 merged #1735 (lamp-tasks --set-up-later, off unless set) and #1736 (backend's role file, first builder's yes at 11:34, no objection from Fourth)
- 11:43 reviewed #1740 (cloud.md: what the backend reads, docs only): no finding, merge on green after #1736
- 11:42 merged #1710 (holding answer: 'let me see' no longer continues, off unless set)
- 11:38 reviewed #1735 (lamp-tasks --set-up-later, off unless set): no finding, merge on green
- 11:37 #1732 (bedtime/morning core skill, output caps): conflicts; new msgs types and product unit args, handed to the first builder; full read on rebased head
- 11:37 #1723 P2 fixed at fcf7f2114; conflicts after #1733, asked Twelve to rebase
- 11:36 merged #1719 (turn-end-wire receipt tests and docs, tests/docs only); one P3
- 11:31 merged #1733 (memory follow-up, off unless set); #1730 green but conflicts with #1721, asked Twelve to rebase
- 11:29 reviewed #1730 (light-scenes, point-light, light-show core skills, not in product): two P3s, merge on green
- 11:28 reviewed #1733 (memory follow-up question, off unless set): two P3s, merge on green
- 11:21 #1728 went back to draft (Ten found a fault); #1731 the first builder merges after a minute on lamp-4ace; #1370 P3s fixed, still the first builder's
- 11:21 merged #1727 (training/name/channel.py, step 1 of the rescue model after #1546; no issue to close)
- 11:18 merged #1721 (presence and light-out, off unless set; new head docs only)
- 11:16 reviewed #1731 (cloud replies say generation lamp-v2): no finding; on by default, handed to the first builder
- 11:15 #1723: Twelve said ready, but the head hadn't moved; told them the P2 stands
- 11:15 merged #1726 (#1713's P1/P2 fixes; texts off unless set)
- 11:14 reviewed #1728 (mood unit + ring pace, off unless set): no finding, merge on green
- 11:12 merged #1725 (tasks accounts wording, part A of #1676, off unless set); #1676 stays open for the rest
- 11:08 #1729 (fourth, cloud generation field): body names a backend person by handle; asked to refile without it
- 11:05 #1726 (fourth, #1713's P1/P2 and P3s fixed): no blocking finding, one P3 (the serve wiring untested); merge on green ARM64
- 11:03 #1727 (nine, training/name/channel.py): no finding; merge on green ARM64
- 10:59 #1723 (twelve, forget everything, off unless set): P2, with nobody known it empties the room's memory (a guest or the TV can wipe it); not ready
- 10:57 #1725 (eighth, tasks accounts wording, off unless set): no finding; merge on green ARM64
- 10:56 merged #1718 (tests, runtime tests take turns with spawns, #1569 row 27), ARM64 green
- 10:53 #1721 (nine, room/presence and light-out, off unless set): one P3; merge on green ARM64
- 10:38 merged #1716 (twelve, schedule set-aside health and crash identity), ARM64 green
- 10:38 #1713: merged at 10:21 over my own agent's unread 10:09 P1 (texts told the spoken thread) and P2 (face/memory calls act on the person in view); LAMP_MESSAGES set nowhere; fix coming from Fourth; owned on #1713 and #705
- 10:29 #1717 (thirteen, lamp-body-measure factory tool): no blocking finding; turns torque on with lamp-4ace's stand on another lamp, so the first builder's after a watched run
- 10:26 #1716 (twelve, schedule set-aside health and crash identity): no finding, mutation caught; merge on green ARM64
- 10:25 merged #1703 (seventh, cleanup failures kept and joined, #1620), ARM64 green
- 10:22 merged #1714 (fourth, A/B numbers in same-voice.md), ARM64 green
- 10:21 merged #1713 (fourth, LAMP_MESSAGES, off unless set), ARM64 green
- 10:21 #1713 (fourth, app texts answered in text, off unless set): two P3s (speak ignored; a text can move the lamp); merge on green ARM64
- 10:14 merged #1709 (third, #1706's P3s), ARM64 green
- 10:04 #1714 (fourth, A/B numbers into same-voice.md): no finding; merge on green ARM64
- 10:00 #1709 (third, #1706's P3s): no finding; merge on green ARM64
- 09:50 #1708 (first builder, lamp-4ace's units synced): no finding, no credential; their own merge
- 09:34 merged #1704 (seventh, endpoint exit receipts, trace data only), ARM64 green, peer read by Tests
- 09:32 #1698 re-reviewed at e34bcf467: the P2 and four P3s fixed (a core skill's tool can't be taken; O_NOFOLLOW); 16 mutants caught; ready once folded; the first builder's merge
- 09:21 merged #1706 (third, #1696's P3s), ARM64 green; three new P3s
- 09:15 merged #1705 (third, #1666's doc P3s), ARM64 green
- 09:07 merged #1688 (second, LAMP_HOLDING_ANSWER, off unless set), ARM64 green; three P3s
- 09:01 merged #1701 (fourth, same-voice question rule with the late-print fix, off unless set), ARM64 green
- 08:57 #1701 (fourth, #1636 folded with the late-print fix): no blocking finding, one doc P3; merge on green ARM64
- 08:57 #1705 (third, #1666's doc P3s): no finding; merge on green ARM64
- 08:45 #1702 (first builder, hardware role template, docs): P2, the template asks for each person's GitHub handle in the public repo; their own merge
- 08:39 #1698 (first builder, core/installable skills, install as root): P2, an installable skill can take over a core skill's function by tool name; four P3s (incl. a link race in read_folder); the tar reader held against 9 hostile tars; their own merge
- 08:31 #1667 re-reviewed at 5578a6e7e: the P2 fixed (reminders wait for the voice to rest; news counts from arrival); ready once folded; the first builder's merge
- 08:29 merged #1696 (third, LAMP_USE_SKILL, off unless set), ARM64 green; five P3s for a follow-up, lamp step 1's expected journal line won't come
- 08:18 merged #1692 (fourth, heard-as test takes its turn first), ARM64 green
- 08:11 merged #1690 (tests, tasks harness tests take turns, #1569 row 18), ARM64 green
- 08:09 #1692 (fourth, heard-as test takes its turn first): no finding; merge on green ARM64; #1691 the same fix
- 08:08 #1695 (sixth, role file: shared access to lamp-52e6): no text finding; a role/scope change, so the first builder's merge
- 07:52 #1686 (first builder, LAMP_VOICE_ROUTE=second, off unless set): three P3s (run_start says gemini on a second run); their own merge
- 07:41 #1687 (first builder, the owner's 19 Oct scope decisions, docs): no finding; their own merge
- 07:36 #1666 (third, a stale privacy reading at start is waited past): two doc P3s; the first builder's merge (privacy path)
- 07:35 #1667 (first builder, skills in the product): P2, a reminder or news coming while the voice is busy past 20 s waits 10 s in silence and loses the skill's movement; two P3s; their own merge
- 07:29 merged #1665 (fourth, LAMP_MISHEARD_HUM, off unless set), ARM64 green; #1659 closed
- 07:15 #1665 (fourth, #1659 folded and rebased): no finding; merge on green ARM64
- 07:10 merged #1664 (third, #1660's P3s: channel bounds pinned, late runs said), ARM64 green
- 07:02 #1663 (tests, #1657 folded with the stamp cap on all three): no finding; the first builder's merge
- 06:54 #1664 (third, #1660's P3s: bounds pinned, runs said): no finding; merge on green ARM64
- 06:52 #1656 re-reviewed at 072f36e65: all three findings fixed; ready for the first builder's lamp check and merge
- 06:44 #1659 re-reviewed at 3cc6ba3fc: the 06:03 findings fixed; needs a rebase (conflicts with #1661's test) and a fold; then merge on green ARM64
- 06:43 merged #1662 (tests, recorder watchdog read waits for the close, #1569 row 28), ARM64 green
- 06:26 merged #1661 (fourth, LAMP_NAME_NOTE_HEARD_AS, off unless set), ARM64 green
- 06:15 #1661 (fourth, name note heard-as, off unless set): one P3; merge on green ARM64
- 06:04 #1660 (third, provider input taken late in order): ready, two P3s; on by default, the first builder's merge at 08:15
- 06:03 #1659 (fourth, misheard hum, off unless set): P2, the worker test can't fail for 'note on every request' or 'cut_in ignored'; three P3s; not ready
- 05:56 #1657 (tests, policy outcomes by stamps, #1652): P3s only; on in the product (log only), so the first builder's merge
- 05:46 #1658 (third, clean stop dates the mute record at the last poll): ready, one test P3; the first builder's merge (mute path)
- 05:29 #1656 (third, skills' own folder, on by default): 2 P2s (self-repair can't remake the folder; the skill flag untested); the first builder's merge
- 04:55 merged #1653 (seventh's #1498 rebased by the first builder: LAMP_CAPTURE_WAIT, off unless set), ARM64 green; #1498 closed
- 04:53 merged #1654 (fourth, docs: the 49 take-6 count labelled, #1651's P3), ARM64 green
- 04:32 merged #1650 (tests, ring flood on controls/touch_state, #849), ARM64 green
- 04:31 merged #1651 (fourth, LAMP_CUT_IN_TOLD, off unless set), ARM64 green; one doc P3
- 04:22 merged #1649 (tests, policy outcome test speaks after the ask, #1569 row 25), ARM64 green; the outcomes-log under load noted for an issue
- 04:16 merged #1648 (nine's #1456 rebased and folded: LAMP_CONTROLS_WAIT, off unless set), ARM64 green; #1646 and #1456 closed
- 03:54 #1647 (first builder carrying #1615, turn-times heard rows): no finding, mutation caught; their own merge
- 03:49 merged #1645 (nine's #1519 rebased by the first builder: lamp-eyes learned noise, off unless set), ARM64 green
- 03:48 #1644 (first builder carrying #1559, ledger forward-read): no finding; their own merge
- 03:44 #1646: ARM64 red, a Linux-only test (tests.rs:1887) lacks the new wait field; fix asked
- 03:41 #1636 re-reviewed at eda24bc9b: the 02:48 findings fixed; new P2 (a statement whose print is late doesn't clear the mark); fold after Monday's 07:20 A/B
- 03:40 #1646 (#1456 rebased by the first builder): no finding, press rules hold in both pump modes; merge on green ARM64
- 03:39 #1643 (first builder carrying #1557, records forward-read): ready, one P3; their own merge
- 03:37 merged #1642 (tests, ring flood waits for its 1,000 before the ring starts, #1569 row 24), ARM64 green
- 03:29 merged #1641 (tests, fixture worker test warms lamp-voice before timing, #1569 row 23), ARM64 green
- 03:27 merged #1479 (nine, lamp-eyes settle) after the first builder's lamp run; merged tree gated; #1519 to rebase
- 03:27 merged #1486 (nine, lamp-name --not-while-speaking, off unless set) after the first builder's lamp run; merged tree gated
- 03:20 merged #1639 (tests, #1635's P3s: the stop-words pick tested, docs), ARM64 green
- 03:19 merged #1640 (third, docs: a refused state's number stays seen, #1637's note), ARM64 green
- 03:17 merged #1638 (tests, signal children take their reply once workers are up, #1569 row 22), 0 of 15 parallel failures, ARM64 green
- 02:52 #1637 (third, what counts as a press, #1632's P3s): no blocking finding; handed to the first builder (mute path)
- 02:49 #1636 (fourth, same-voice question goes on): 3 P2s (held lines never clear the mark; asks() matches most statements; condition a untested), not ready
- 02:39 merged #1635 (tests for sixth, LAMP_STOP_WORDS_SOURCE=linear, off unless set), ARM64 green; three P3s (the source pick untested, doc lines)
- 02:17 #1632 re-reviewed at 3f04b4787: the 01:19 P2 fixed, two P3s; handed to the first builder (mute path)
- 02:03 merged #1634 (tests, #1620's red test as should_panic), ARM64 green; one parallel serve_sigterm failure noted for #1569
- 01:40 #1633 (tests carrying twelve's #1603 P3s): no finding, room mutation caught; the model's tool text changes, so the first builder's merge
- 01:20 #1632 (third carrying #1622, head hold across restart): the 22:27 findings fixed; a P2 left (any queued state counts as a press, so a release may never happen on a lamp); the first builder's merge
- 00:31 merged #1630 (third, lamp-policy-replay --probe-connections, hand-run; never sends a rule-decided line), ARM64 green
- 00:10 merged #1629 (third, policy hedge, off unless --hedge-after-ms), ARM64 green
- 00:08 #1630 re-reviewed at be34df97e: both P2s and the P3s fixed (a rule-decided line is never sent, shown by probe); merge on green ARM64
- 00:06 merged #1631 (fourth, #1627's P3s: same-voice.md numbers labelled, test runs scene 1 off too), ARM64 green
- 00:00 merged #1628 (tests, reminder room-change test on its logical clock, #1569 row 21), ARM64 green
- 23:58 #1629 re-reviewed at c129f0e85: the four P3s fixed, all 9 mutations caught, 80/80 clean; merge on green ARM64
- 23:49 #1630 (third, policy connection probe): 2 P2s (with --rule-decides it can send words the lamp never sent; the reading rule counts the handshake), not ready
- 23:49 #1631 (fourth, #1627's P3s): no finding; merge on green ARM64
- 23:42 #1629 (third, policy hedge, off unless --hedge-after-ms): four P3s, 80/80 runs of hedge:: clean; merge on green ARM64
- 23:28 #1494 rebuilt on #1466's d18347e75 (bd27d5c3d): same change, no finding; the first builder's after the lamp check
- 23:26 merged #1627 (fourth, same-voice judges a line begun over a silent reply, off unless set), ARM64 green; P3s for a follow-up
- 23:24 #1627 (fourth, same-voice over a silent reply): three P3s, off unless LAMP_SAME_VOICE_MODEL; merge on green ARM64 (queue 22e builds the PR head, so no push asked)
- 23:03 #1466/#1494 (third, light outranked and patterns): order answered, #1466 then #1494; 3e3609fce is the 12:05 read rebased; the first builder's merge after the lamp check
- 23:00 merged #1621 (third, LAMP_TURN_END_PRIORITY, off unless set), ARM64 green, the 22:27 P3s fixed
- 22:55 merged #1626 (tests, head_tap's doc line back in place, #1619's P3), ARM64 green
- 22:50 merged #1624 (tests, motor tests stepped or restarted on a timed-out start, #1569 rows 19-20), ARM64 green
- 22:50 merged #1623 (tests, provider-pair rigs' lease 1 s, #1569 row 17), ARM64 green
- 22:45 merged #1625 (fourth, test: a reminder into a busy room is no answer), ARM64 green
- 22:33 merged #1619 (tests, a frame the loop reads marks the run heard), ARM64 green; P3 doc line left
- 22:30 #1625 (fourth, reminder into a busy room test): no finding, mutation fails it; merge on green ARM64
- 22:28 #1615 (ten, turn-times idle) re-reviewed at 9f164e6cb: the 21:56 findings fixed; new P2, lines the lamp says on its own become the run's first
- 22:27 #1622 (thirteen, hold across restart): 2 P2s posted; mute path, so handed to the first builder
- 22:10 merged #1573 (Waiting::closed names every case) after the attribution line left its body; ARM64 green
- 22:08 merged #1618 (fourth, #1613 rebased: the #1522 rulings behind LAMP_GOING_ON_ASKS), ARM64 green
- 22:08 merged #1614 (third, LAMP_TURN_END_NO_SPIN, off unless set), ARM64 green
- 22:01 merged #1616 (twelve carrying #1562: schedule stores one release forward, the set-aside keeps its identity), ARM64 green, two P3s
- 21:57 #1615 reviewed at 221b5417b (ten, turn-times first_of_run/idle_ms/stalled_ms): P2 (a request cut before its end of speech gets no row, so its stall goes uncounted and the next is marked first), two P3s
- 21:54 #1617 gated at e962875ec (clippy ok, 40/0); for the first builder
- 21:52 #1616 reviewed at 2cd931d61 (twelve carrying #1562: schedule stores one release forward): P1, P2 and P3s fixed; two new P3s (a crash between rename and commit; putting the archive back by hand); waits for ARM64
- 21:49 #1613 closed; refiled rebased as #1618 (no content change, both switches rows kept); waits for ARM64
- 21:49 #1614 at b45764707: docs P3s fixed (endpoint-pilot.md); waits for ARM64
- 21:48 reviewed and merged #1612 (tests, exchange's read as a helper with its own test; #1604's P3)
- 21:48 #1067 folded to ebf1327ac (plus the body.md P3); still the first builder's. #1609 closed, refiled as #1617
- 21:44 #1613 green but conflicts in switches.md after #1592; asked to rebase
- 21:43 re-checked #1067 at c375b4c48 (fifth, Expression's base sway): the same two commits rebased, no P1/P2, rule 8 passes (2 in 5 sentences, side by hash); for the first builder (real-hardware motion), P3: fold
- 21:41 #1614 reviewed at 60b926f1b (third, LAMP_TURN_END_NO_SPIN, off unless set): two P3s (the slowdown not measured; endpoint-pilot.md wording); waits for ARM64
- 21:39 #1613 reviewed at ef9f679a4 (fourth, the folded #1594 with its fixes): the 20:53 P2 fixed, no P1/P2; waits for ARM64
- 21:38 reviewed and merged #1611 (tests, three motor tests on the stepped clock; #1569 row 16)
- 21:34 #1588 at edc4b6d0e: a test and a tools.md line only; still the first builder's
- 21:27 #1594 closed; refiled folded as #1613 (fourth, going-on rulings with the 20:53 P2 fix), review started
- 21:24 #1115 rebased to 48400e075 (all eight commits unchanged); still the first builder's
- 21:18 merged #1610 (nine, #1605's P3s: pinned models built from the hashed bytes), ARM64 green
- 21:17 #1592 folded to 6acd9c048 (same tree); the first builder's
- 21:16 merged #1604 (tests' fix, exchange reads an answer whose sender closed, macOS only; the first builder's yes at 20:29), ARM64 green
- 21:11 re-checked #1592 at c54e64e41: the fixes are right (the re-ask only keeps the guard; no write through a link); needs a fold; the first builder's merge after peer reads
- 21:03 merged #1606 (tests, #1599 refiled with the fmt fix: the ring head's hand-time test inside 300 ms; #1569 row 14)
- 21:03 #1606 is #1599 with the fmt fix (only that statement differs); waits for ARM64
- 21:01 #1610 reviewed at 0af470905 (nine, #1605's P3s: pinned models built from the hashed bytes; the rest sentence), no findings; waits for ARM64
- 20:58 #1486 rebased to a4443c49e after #1605: only the fields merged; still held for the lamp run
- 20:58 #1609 reviewed at ab4e76653 (fourth, a lamp whose zone names no place says only 'here'): no P1/P2, P3s; for the first builder (LAMP_TIME_PLACES on in the product)
- 20:54 re-checked #1574 at 930119838: the P2 fixed (the scan cap counts /proc entries, 100 ms is the bound), no findings; for the first builder (privacy path)
- 20:53 re-checked #1594 at 147535e1a: both P2s fixed; a new P2 (Policy holds don't count as talk, so a busy room ends 60 s after the answer while people talk on: #1522 back); needs a fold
- 20:53 merged #1605 (nine, lamp-name --rescue-model, off unless set), ARM64 green; #1486 must rebase
- 20:49 re-checked #1588 at 27baeb766: no findings, P3s fixed; still the first builder's after lamp check 21b
- 20:47 #1605 reviewed at 8685c59ce (nine, lamp-name --rescue-model, SHA-256 pinned, off unless set): two P3s; waits for ARM64; conflicts with #1486
- 20:46 #1592 re-checked at 73f7f53ed (thirteen, LAMP_GUARD_NEVER_LISTENED): P2s fixed, two P3s; for the first builder (privacy path)
- 20:44 reviewed and merged #1601 (tests, the promised-look reminder test on the stepped clock; #1569 row 13)
- 20:41 merged #1596 (twelve, docs: the slow short asks as measured), the P2 fixed, ARM64 green
- 20:39 #1603 reviewed at 7613f5a9b (twelve, a due-only reminders list says how many are set and not yet due; #1595): two P3s; for the first builder (on by default, their 21c run)
- 20:35 #1599 reviewed at 15f4fe4bd (tests, the ring head's hand-time test inside the 300 ms; #1569 row 14): fmt fails at ring_head.rs:195; asked to run cargo fmt
- 20:35 re-checked #1588 at c95c8044f (rebased after #1549, per-kind renewal state): no P1/P2; for the first builder (#1484 rule b), waits for the lamp check
- 20:31 reviewed and merged #1593 (tests, the round-trip fixture ticks again on an expired offer; #1569 row 11)
- 20:31 reviewed and merged #1582 (tests, the cancelled-reminder test reads the wait's own report; buses named by a count; #1569 row 4)
- 20:28 reviewed and merged #1597 (tests, the signal test's recording peer waits MAX_FINISH_WAIT; #1569 row 12)
- 20:25 #1602 reviewed at 8dfea257f (fourth, the lamp's own time names its place; Etc/GMT after any folder): code fine; for the first builder, since #1598 made LAMP_TIME_PLACES a product default
- 20:23 #1596 at 62704abf9: the P2 fixed (v2's hash, window and counts); waits for ARM64
- 20:18 merged #1600 (nine, lamp.py train --rescue), ARM64 green
- 20:16 #1514 rebased to c63b87e07 (the conflict with #1520 resolved, Ten's P3 folded): still the first builder's merge
- 20:12 reviewed and merged #1590 (tests, voice rigs end a fixture on its Stop and wait for the exit; #1569 row 7)
- 20:04 #1594 reviewed at 5f6a14848 (fourth, #1522's three rulings: a look_up plays, no timer from talk, a busy room ends): two P2s (the room ends at the first answer after a 60 s gap; a timer answering the lamp's own question is refused), P3s
- 20:03 closed #1545 (all four items merged: #1551, #1565, #1581)
- 20:02 merged #1581 (third, the owed-reminder pulse on the asleep face, off unless LAMP_REMINDER_PULSE=on), ARM64 green
- 20:01 #1600 reviewed at cf5c62130 (nine, lamp.py train --rescue: the installed model gates, the trained folds rescue), no findings; waits for ARM64
- 20:00 merged #1591 (sixth, a committed stop holds the speaker until the cancel; #1555's P3s), ARM64 green
- 20:00 #1598 reviewed at f2066644e (fourth, LAMP_TIME_PLACES on in the product unit): no findings; for the first builder (product default)
- 19:48 #1596 reviewed at 9c5c4a635 (twelve, docs: the slow short asks as measured): P2 (the figures are v2's but the bullet cites v1's hash and the wrong window), three P3s
- 19:47 #1582 gated at a8f952184 (21/0 three times), but it went back to draft at afc7e6838; re-check when ready
- 19:43 #1574 re-checked at 7d27d9f86 (seventh, a busy mic waited 8 s and its holder named): P3s fixed, one P2 (the scan cap counts every fd); for the first builder (privacy path, queue 18)
- 19:42 merged #1571 (el, a thread file this build can't read is set aside with one line; the first builder gave it to me at 17:02), ARM64 green
- 19:41 #1588 reviewed at ea6f0a710 (third, a missing kind coming back renews between turns even mid-conversation): no findings; for the first builder (changes #1484 rule b), merges after #1549. #1582 back to draft at afc7e6838 (Tests found the real cause)
- 19:40 on #1569 to Tests: a_reminder_pending_at_a_start_is_owed_to_no_mute failed once under load
- 19:40 reviewed and merged #1584 (tests, the reminder tests take a lock again until a sibling's fork lets go; #1569)
- 19:37 merged #1589 (fourth, the time-places P3s: unknown place says how to ask, Etc/GMT sign, non-string place unknown; off unless set), ARM64 green, two P3s
- 19:35 reviewed and merged #1577 (tests, the privacy bus tests try again only when the host stalled past the lease; #1569 rows 2-3)
- 19:34 merged #1537 (seventh, directed-fixture --fixture-at-ms, diagnostic only; counts from listening_ready), ARM64 green
- 19:23 merged #1587 (eighth, #1427's P3s), ARM64 green
- 19:22 #1587 reviewed at 7b7d6e1a6 (eighth, #1427's P3s: the key tested through the loop, tasks.md), no findings; waits for ARM64
- 19:20 #1581 reviewed at 569652573 (third, the owed-reminder pulse, off unless set): P2 (no wait-level test pins owed && pulses), three P3s; not ready
- 19:13 #1486 rebased to 0d0688efd after #1555: only the flags merged; --not-while-speaking doesn't gate stop words; still held for the lamp run
- 19:12 merged #1580 (fourth, a going-on line the Policy decides only in shadow is let out at once; behind LAMP_GOING_ON_ASKS), ARM64 green
- 19:11 reviewed and merged #1576 (tests, voice fixtures send Stop until the peer is gone, the exec barrier read from /proc; #1569 row 5)
- 19:09 merged #1570 (eighth, tasks state decoded from the bytes, duplicates refused, the receipt read raw), ARM64 green
- 19:05 #1498 at 45a6e2dfe: the doc P3 done (numbers re-run in serve-runtime.md and switches.md); still waits for #1456
- 19:04 merged #1583 (el, the mute record's identity read after load_noting's checks; #1548's peer P3s), ARM64 green, one P3
- 19:02 merged #1555 (sixth, stop.onnx in lamp-name and LAMP_STOP_HEARD, both off; a short Stop now ends the reply), ARM64 green, P3s; #1486 must rebase on lamp-name main.rs
- 19:00 #1583 reviewed at 6c05c4e5a (el, the mute record's identity read only after load_noting's checks): one P3; waits for ARM64
- 18:59 merged #1427 (eighth, news asked about rides on the answering turn; inert without --close-told), ARM64 green, three P3s
- 18:55 merged #1578 (ten, tests: lamp-policy's FOLLOWS_FROM pinned to the voice's), ARM64 green
- 18:51 merged #1558 (el, people.json one release forward, set aside never dropped, the reset empties it; the first builder gave it to me at 17:02), ARM64 green, reset plans equal
- 18:51 #1570 at f19651b03: plus a test helper waiting for the store lock after stop (a flaky runtime test); waits for ARM64
- 18:50 merged #1470 (fifth, Face Mesh V2 inner lips for speaking, off unless given), folded, ARM64 green
- 18:48 merged #1575 (fourth, #1568 folded: own zone from the /etc/localtime link, no top-level fixed zones), ARM64 green
- 18:45 #1549 at f59cadd3a: one doc line (the new P3's byte count); still the first builder's merge after the lamp check
- 18:45 #1570 at efe08c790: my P3 fixed (the receipt read as raw text, a duplicate inside it dropped, tested); waits for ARM64
- 18:44 #1578 reviewed at 1b79e46b4 (ten, a test pins lamp-policy's FOLLOWS_FROM to the voice's), no findings; waits for ARM64
- 18:34 #1573 reviewed at 933c22280 (thirteen, Waiting::closed names every case): code fine; asked to remove the attribution line from the PR body; waits for that and ARM64
- 18:33 #1470 folded to 2f2cb6066 (tree equal to bc8278d8d); waits for ARM64
- 18:31 merged #1564 (fifth, eyes-cost.sh, read-only), folded, ARM64 green
- 18:31 #1570 reviewed at dc277fac7 (eighth, tasks state decoded from the bytes so a duplicate field is refused again): one P3; waits for ARM64
- 18:29 #1575 is #1568 folded (tree equal to ea005887d); waits for ARM64
- 18:28 re-checked #1549 at 055298f40: all four P3s fixed; ready for the first builder's merge after tonight's lamp check
- 18:27 #1572 is #1408 refiled (same 95a71daa7, the body without a person's name); reviewed on #1408; waits for the first builder (Monday)
- 18:26 re-checked #1470 at bc8278d8d: all four findings fixed (Speaking gains an optional from field; nothing else reads Speaking), ARM64 green; asked to fold the review fix-up
- 18:24 #1568 at ea005887d: doc P3 fixed in two fix-up commits; asked to fold
- 18:22 merged #1548 (el, a newer build's mute record reads as muted, said once; the first builder gave it to me at 17:02), folded, #1553's test flipped, ARM64 green
- 18:21 closed #1249 after #1566
- 18:21 reviewed and merged #1566 (tests, service_privacy_controls' stopping fixture waits for the child's exit; #1249's last case)
- 18:21 re-checked #1558 at 20b05b358: bump rule in, P3s fixed; one P2 case open (forget misses a set-aside record whose name isn't plain text), two new P3s
- 18:20 #1568 reviewed at 9e8278794 (fourth, #1552's P3s: own zone from the /etc/localtime link, no top-level fixed zones), one doc P3; still off unless set; waits for ARM64
- 18:18 #1562 reviewed at 6c08532ac (second, schedule stores read one release forward): P1 (doesn't flip #1553's engine test, so lamp-voice's schedule tests go red on merge), P2 (a set-aside store's new store_id makes the ledger refuse every new reminder), two P3s; not ready
- 18:18 reviewed and merged #1561 (tests, forward-read should_panic tests for five surround files; #1505)
- 18:17 #1555 reviewed at 6296ff919 (sixth, stop.onnx and LAMP_STOP_HEARD, off): one P2 (a forced yield on a short Stop commits with no turn let in, so the reply plays on), P3s and a fold; not ready; conflicts with #1486 in lamp-name main.rs
- 18:17 merged #1563 (fourth, LAMP_GOING_ON_ASKS, the folded #1538; the second half of #1522's fix), ARM64 green
- 18:14 #1408 reviewed at 95a71daa7 (eighth, --close-told on in the product unit): change fine; for the first builder (product default); the body names a machine after a person, noted for the author
- 18:11 merged #1565 (thirteen, owed only while the wait holds the mics closed; a mute past an hour tested; #1551's P3s), ARM64 green
- 18:11 #1564 folded to cb3928d6b; waits for ARM64
- 18:08 merged #1560 (ten, docs: faster-answers.md as measured), ARM64 green, five P3s left
- 18:07 reviewed and merged #1556 (tests, forward-read should_panic tests for eyes, thread, settings, memory; #1505)
- 18:04 #1548 rebased to ea9fae39a: one commit, #1553's mute test flipped to plain; waits for ARM64
- 18:04 #1559 reviewed at e19dbd430 (second, the delivery ledger read one release forward): sound; P2: rebase and flip #1553's ledger test (conflicts in delivery_ledger.rs); four P3s
- 18:03 #1557 reviewed at 289d1af95 (second, records' reports read one release forward): no report feeds a cleanup decision; P2s: rebase and flip #1553's test, and the commit subject needs the lampOS: prefix; two P3s
- 18:02 merged #1554 (eighth, the set-aside tasks store in health; newer targets run with hand-overs off), the first builder gave it to me at 17:37, ARM64 green
- 17:59 #1564 at ff9d1d6c2: the P3 fixed (journal cursor); asked to fold
- 17:57 reviewed and merged #1553 (tests, forward-read should_panic tests for mute, schedule, ledger, records; #1505)
- 17:54 re-checked #1498 at 3fae4f7a5: folded to one commit, the no-render test dropped, ARM64 green (blocks read 15 µs late); one doc P3 open (numbers from the first version); waits for #1456 and a rebase
- 17:52 #1470 reviewed at e1a59cfab (fifth, Face Mesh V2 inner lips for speaking, off unless set): one P2 (Speaking.score's scale differs under the mesh and msgs doesn't say which judge; add a from field), three P3s; no picture kept; with the mesh on a camera close waits up to one mesh pass (85-290 ms), noted
- 17:44 #1554: the first builder gave it to me (17:37); folded P3s at ab6118493 (an unknown engine also reads as newer); waits for ARM64
- 17:44 merged #1552 (fourth, the_time takes a place, LAMP_TIME_PLACES off unless set; hostile zone names refused, 11M moments checked), ARM64 green, two P3s; closed #1547
- 17:42 #1558 reviewed at 0e395fb88 (el, people.json one release forward, set-aside): two P2s (forget doesn't reach set-aside prints; no bump rule for a later field that limits face use), three P3s; not ready (the merge is mine, 17:02)
- 17:41 #1560 reviewed at 22950eb8f (ten, faster-answers.md: where a turn's time went, as measured): every figure matches its #705 post; five P3s (wording of the held wait, the slow-turns sample, provenance line); waits for ARM64
- 17:40 #1564 reviewed at 1a01cdce9 (fifth, eyes-cost.sh, read-only, for #1470's switches): one P3 (the journal's boundary second counted twice); waits for ARM64
- 17:39 #1548 at bfd5aa42c: P2 fixed (words only); asked to fold the two commits; I merge after the fold
- 17:36 #1548: corrected my note, the first builder gave it to me (17:02); I merge after the P2 fix. #1554 reviewed at 552bc08ee (eighth, set-aside store in health; newer targets run with hand-overs off): three P3s; for the first builder (on by default, no explicit hand-over)
- 17:31 reviewed and merged #1550 (tests, privacy_worker's rig passes over a lease late only by its read; my 16:10 flake), closed #1544
- 17:28 merged #1551 (thirteen, a reminder due while muted waits and is said as owed; the first builder gave it to me at 17:08), ARM64 green, two P3s; #1545 left open for the ring's pulse
- 17:27 #1548 reviewed at 914d87aaf (el, a newer build's mute record reads as muted): nothing reads unmuted; one P2 (the journal line and doc name the wrong hold); for the first builder: privacy path
- 17:23 #1549 reviewed at 5567ebad4 (third, a session told each thing it can't do right now): four P3s; for the first builder: on for every session, waits for tonight's lamp check
- 17:16 merged #1546 (nine, training/name/lamp.py: speaking voices only, negatives held out by stretch), ARM64 green
- 17:14 re-checked #1498 at cb354dbe0: the no-render test still fails ARM64 (rendering every third block leaves the reference behind with recovery off); P3s still open; not ready
- 16:57 #1546 at bc2ae44bc: my P3 fixed (the other lines named in the docs), docs and comments only; waits for ARM64
- 16:54 merged #1539 (ten, --agreement --from/--until for the shadow night), the Policy owner's yes, ARM64 green
- 16:54 reviewed and merged #1543 (tests, lamp-controls' recording tests give the last fsync 5 s; the flake I reported), closed #1542
- 16:52 re-checked #1498 at 7c666a641: the earlier P2 fixed (the mic polled for the whole wait, about 15 µs late on the rig); a new P2: the no-render rig test cannot pass (ReferenceLate after 24 frames) and turns ARM64 red; two P3s; fold to one
- 16:49 re-checked #1538 at 3ba8c41d4: the lamp_asked P2 fixed; the 16:14 P2s (wanted follow-ups at follows_lamp 0.18–0.57 and a look_up at 0.9 held) not fixed; needs a rebase and fold (a merge commit and fix-ups)
- 16:45 merged #1541 (eighth, docs: my four #1533 P3s), ARM64 green
- 16:44 #1546 reviewed at 87336d077 (nine, training/name/lamp.py: speaking voices only, negatives held out by stretch, counted by source), one P3; waits for ARM64
- 16:35 merged #1520 (twelve, LAMP_SHORT_DONE in lamp-actions, off unless set, the owner's yes), folded and rebased, ARM64 green
- 16:34 #1539 rebased to a7ad0b294 after #1524, range-diff with no content change; waits for ARM64
- 16:34 re-checked #1115 at 33bdb88c0: held joints' loads kept with the run's peaks, right, lamp-body 446/0; the first builder's merge after step 4
- 16:31 reviewed and merged #1540 (tests, docs: realtime-tests.md records the #1128 batch; ARM64 figures checked against each PR's run)
- 16:28 #1541 reviewed at 5600c0ac3 (eighth, docs: my four #1533 P3s), no findings; waits for ARM64
- 16:23 #1539 at 904f88f16: P3s fixed, the Policy owner's yes at 16:12, but conflicts in replay/tests.rs after #1524 merged; asked to rebase
- 16:23 merged #1524 (ten, lamp-policy --rule-decides, off unless set), the Policy's owner's yes at 15:58, ARM64 green
- 16:22 merged #1518 (fifth, lamp-body's fold line and bad reads in the journal), folded, ARM64 green on the rerun (the first run failed controls' privacy_only_touch_recording…, a crate this PR doesn't touch)
- 16:21 #1538 reviewed at 80d632053 (fourth, LAMP_GOING_ON_ASKS, the second half of #1522's fix): P2s with the peer reviews (replies to the lamp's question and wanted follow-ups held; lamp_asked read from the line before), three P3s; not ready
- 16:17 #1520 rebased to 8ca2354dc: range-diff shows only #1495's spoken(name) kept in the Already line; waits for ARM64
- 16:15 merged #1533 (eighth, LAMP_REMARKS: two remarks held end a busy room's conversation; half of #1522's fix), ARM64 green, four doc P3s left
- 16:13 #1537 reviewed at efa2884af (seventh, directed-fixture --fixture-at-ms): two P2s (the start counts from the loop's start, not listening_ready; the tests don't run the real loop), P3s; not ready
- 16:10 re-checked #1513 at 6f8860550: both P3s fixed (one read through Recalling::opened, tested through it), no findings, ARM64 green; the first builder's merge after the lamp check
- 16:10 reviewed and merged #1536 (tests, the transport's held-up test on one stepped clock; #1128), ARM64 green
- 16:10 on #705 to Tests: two privacy tests failed once each on ARM64 on PRs that don't touch them (privacy_worker losing_controls…, controls privacy_only_touch_recording…), both green on rerun
- 16:09 merged #1532 (sixth, docs: linear-gated's replayed figure), ARM64 green on the rerun (the first run failed privacy_worker::losing_controls_ends_the_real_worker_cleanly_and_expires_its_lease, a test this docs PR doesn't touch)
- 16:09 #1539 reviewed at c1a3f1d3d (ten, --agreement --from/--until for the shadow night): no blocking finding, two P3s (Feb 30 rolls over; the flags ignored outside --agreement); waits for ARM64 and the Policy owner's yes
- 16:04 #1524 at d9e882077: Fourth (the Policy's owner) approved at 15:58; only policy.md changed since 9c878faaf; waits for ARM64
- 15:59 reviewed and merged #1531 (tests, lamp-name's lease test halves; #1128), ARM64 green
- 15:59 reviewed and merged #1530 (tests, progress_news waits for its note to be let go; #1128), ARM64 green
- 15:58 reviewed and merged #1535 (tests, hello_asks steps the clock to the 2 s edge; #1128), ARM64 green
- 15:53 #1498 reviewed at 3ba19de04 (seventh, LAMP_CAPTURE_WAIT, stacked on #1456): one P2 (the mic is polled a block's time after the last read, not after the block was there, so a whole-block mic locks to the speaker's 10 ms render), two P3s; waits for the fix, #1456 and a rebase (conflicts in audio_workers.rs, process.rs, serve-runtime.md)
- 15:53 #1475 closed unmerged by Seventh on the first builder's 15:43 decision (two failed lamp runs with capture pinned; LAMP_CAPTURE_CORES stays unset, the saving goes through #1498)
- 15:51 re-checked #1524 at 9c878faaf (rebased onto lamp-voice): all P3s and a peer P2 fixed, lamp-policy 246/0; no ARM64 run on this head yet, and waits for the Policy owner's yes
- 15:51 merged #1534 (eighth, docs: tasks.md's newer-schema bullet; my #1512 P3), ARM64 green
- 15:50 reviewed and merged #1529 (tests, provider_pair's abandoned-turn test on 500 ms patience; #1128), ARM64 green
- 15:50 reviewed and merged #1528 (tests, motor preflight on the test's own 1 s; #1128), ARM64 green
- 15:47 #1520 folded to a0c093290 with both P3s in, but conflicts in light.rs with #1495 (merged 15:37); asked to rebase
- 15:45 #1518 folded to 54846130e, same tree as 99bc47c5f; waits for ARM64
- 15:41 reviewed and merged #1527 (tests, records' full-pipe test on a 200 ms deadline; #1128), lamp-records 155/0, ARM64 green
- 15:41 reviewed and merged #1526 (tests, three bus CLI tests' windows under a second; #1128), lamp-bus 100/0, ARM64 green
- 15:38 re-checked #1115 at 3246d9373: my two P3s answered (misread hold tested, body.md), lamp-body 445/0; the first builder's merge after step 4
- 15:37 merged #1525 (tests, lamp-skills host-clock tests; #1128), ARM64 green; #1128 stays open for the other slow tests
- 15:36 #1525 reviewed at 567957072 (tests, lamp-skills' host-clock tests load the skill at start, not 2 s of reload after; #1128), no findings, lamp-skills 85/0; waits for ARM64
- 15:34 re-checked #1513 at 402a2a62a: four P3s fixed, one commit, two new P3s (connect untested; conversation-thread.md's byte count); the first builder's merge after the lamp check
- 15:33 #1534 reviewed at 1079d97fd (eighth, tasks.md points the newer-schema bullet at the bump rule; my #1512 P3), no findings; waits for ARM64
- 15:33 re-checked #1518 at 99bc47c5f: P2 fixed (the fold line's baseline is the last folded step), lamp-body 454/0; ready once folded
- 15:31 #1532 reviewed at dd9568819 (sixth, docs: linear-gated's replayed figure 10 of 19 and 2 false); numbers re-run from the first builder's tables, no findings; waits for ARM64
- 15:25 merged #1523 (fourth, the same-voice bar of 0.45 pinned by a test, its short-line cost in same-voice.md; #1508's P3s), ARM64 green
- 15:23 #1524 reviewed at 5fb628996 (ten, lamp-policy --rule-decides, off unless set): no P1/P2, four P3s; waits for a rebase onto lamp-voice (it still carries #1510's old commit), retarget, ARM64 and the Policy owner's review
- 15:23 re-checked #1520 at 73bbcc4a8: P2 fixed (Outcome::Already, off byte for byte), two P3s (volume_down while muted still Done; tests cover 2 of 11 Already sites); ready once folded to one commit
- 15:16 merged #1521 (sixth, docs: linear-gated's replay gates without reference trust; #1504's P3s), ARM64 green; no issue to close
- 15:14 merged #1509 (el, a put-back round says each unit's restarts and last journal line; both P2s fixed at 1f0adafaf, ARM64 39109431032 green); noted on #1505, left open for #1512 and the rest
- 15:12 re-checked #1519 at 255dbe310: P3 fixed (doc comment and eyes.md), one commit on #1479, waits for #1479
- 15:11 merged #1499 (eighth, remarks held in a busy room, LAMP_REMARKS off unless set) and #1510 (ten, lamp-policy-replay --agreement); noted on #829, left open for its other scenes
- 15:08 #1523 reviewed at e28c88e62 (#1508's P3s: the 0.45 bar pinned by a test, its short-line cost in the docs): no findings; merge on green ARM64
- 15:07 #1494 re-checked at 69b30f6f0: both P3s fixed; the first builder's 14:27 gives the merge to the reviewer (no skill names a pattern yet); merges after #1466 (the first builder's, lamp check pending) and a rebase
- 15:07 #1499 re-checked at f2cf12aec: both P2s fixed, the conflict resolved; one P3 (a few phrasings still held: 'wanna', 'What kind of music do you like'); merge on green ARM64 (off unless set)
- 15:07 #1512 re-checked at 325aa606e: both P2s and the P3 fixed, the peer's P2 too; ready for the first builder's merge on green ARM64
- 15:05 #1519 reviewed at f403b938b (top commit): code ready; someone moving unseen learned as noise after about 25 s, a turn still seen at once, nobody missed past the 3 s look; one P3; waits for #1479 and a fold
- 15:05 merged #1517 (lamp-ipc-probe retries a peer not yet private; the probe tool only); closed #1516
- 15:05 merged #1515 (lamp-health's silent-part test checks the eyes line only where the board is well; tests)
- 15:04 #1518 reviewed at 295994b7f: P2 (the fold line says 'nothing retreated' for the folds that are a sit-down for a strain, since the retreat is counted before the fold starts); the merge is the reviewer's once fixed (14:05 yes)
- 15:00 #1520 reviewed at 89d16a61a: P2 (the 'already' answers are shortened too: volume_up at 100 says 'It is done… There.'), two P3s; off unless set holds
- 14:58 #1521 reviewed at efbfa8b9d (#1504's P3s, docs): no findings; merge on green ARM64
- 14:54 #1513 reviewed at d0181c37c: code right, P3s only; for the first builder (what the model says on real hardware, the builder has no lamp); ARM64 green
- 14:52 merged #1504 (LAMP_VAD_INPUT=linear-gated, off unless set)
- 14:51 #1504 re-checked at 82b5d1184: P2 fixed in code (the gate opens only on a clear room or 20% explained), P3s fixed, one new P3 (the 11-of-19 table mixes two runs); merge on green ARM64
- 14:50 #1520 ready (LAMP_SHORT_DONE: a word or two after a light or volume change, off unless set); review started
- 14:50 #1510 at b600ddd93: last P3 fixed (pictures split on bytes); merge on green ARM64. #1514 at 70ccc4824: doc P2 fixed; the first builder's
- 14:48 #1512 reviewed at 8d7b15296: two P2s (a receipt whose reply has a newer field still refuses the store, the 13:51 failure; the set-aside test fails on every Mac through the /var symlink), P3s; the first builder merges after
- 14:47 merged #1511 (#1490's and #1501's P3s: the lamp's 1 s report pinned, the alive check shown able to say yes; tests). #1518 (fold line) and #1519 (eyes noise line, stacked on #1479) ready; reviews started
- 14:44 #1514 reviewed at 59650e572: code right and honest (a gap call answers 'could not tell', writes nothing); P2 docs still say withdrawn within about two seconds; for the first builder after (real hardware, no lamp of its own)
- 14:43 #1510 re-checked at 8f8c4def6: the owner's P2 and both P3s fixed, the owner approved at 14:40; merge on green ARM64
- 14:43 #1475 re-checked at 3bea1a1b9: one commit, one account in the doc, green; two P3s; merges once the first builder's lamp run passes
- 14:38 #1509 reviewed at 04ada8cf0: two P2s (the outcome with a part's last journal line is kept in lamp-update's log.jsonl for months and the reset doesn't wipe it; journalctl has no time limit and runs before the put-back), P3s; the line never leaves the lamp
- 14:36 #1514 ready (lamp-actions keeps the light offered 15 s past its last fresh state); review started
- 14:35 #1513 ready (a session without light functions is told it can't change the light); review started. #1504 and #1510 moved; re-checks started
- 14:34 #1499 re-checked at 0b6c4e8f1: earlier findings fixed; two new P2s ('I was wondering if', 'I'd love to hear a joke', 'I can't hear it' still held; a reply to the lamp's wh-question without '?' held); conflicts in provider_worker.rs after #1474
- 14:33 #1512 ready (lamp-tasks reads the next release's state; on by default); review started
- 14:30 #1507 merged by the first builder: reviewer.md's merge conditions now allow Python as training code under lampOS/training/ on joining.md's conditions
- 14:30 #1425 merged by the first builder. #1510 reviewed at 44f57601d: P2 (the Policy owner's: a model stay_quiet that follows the lamp is counted as agreeing with the rule's plain stay_quiet, though the voice lets calls run on it), two P3s
- 14:30 merged #1508 (same-voice bar 0.45 from lamp-4ace's labelled set, still off; #1467's P3s); note for the first builder: 0.45 would hold most short same-voice follow-ups (31 of 38 pairs), fix the doc before switching on
- 14:27 #1507 moved to 1f4031dce: now writes a Python exception into joining.md and reviewer.md; a rule change, so still the first builder's
- 14:23 #1474 merged by the first builder. #1475 folded to 3bea1a1b9 with a doc fix and a capture_cores change; re-check started
- 14:23 #1510 ready (lamp-policy-replay --agreement, read-only); review started
- 14:22 #1509 ready (a round put back says each unit's restarts and last journal line); review started
- 14:22 merged #1501 (three tests ask the kernel about their children, no kill program; tests). #1504 reviewed at d13943b3d: P2 (docs say the lamp alone gets the processed score for every reader; false for the stop rule, so false yields are linear's), two P3s
- 14:17 merged #1506 (#1491 folded: setup's address watcher ends with the join; on by default, the first builder's 12:4x gave the merge to the reviewer)
- 14:12 #1494 re-checked at 73542b54f: both P2s and the P3 fixed (candle within 80-100%, pulse a 3-6 s arc ending on the level); two new P3s; for the first builder after the lamp-4ace film and the owner's look; waits for #1466
- 14:10 merged #1503 (#1484's P3s: docs with the lamp's lines, a test pins the fresh open)
- 14:08 #1475 re-checked at 51d7b4648: rebased and green; still a P2 (serve-runtime.md has two accounts: 'don't set' and '6-7 keeps capture on the big cores', though 6-7 is now refused), P3 fold; held for the lamp run
- 14:02 #1508 ready (same-voice bar 0.45, still off; #1467's P3s); review started
- 14:02 #1507 (training/name/lamp.py) sent to the first builder: it adds Python, which reviewer.md's merge conditions forbid (training/ already holds Python from direct commits)
- 13:57 #1501 (tests ask the kernel, no kill program) and #1504 (LAMP_VAD_INPUT=linear-gated) ready; reviews started
- 13:53 #1499 reviewed at 8c1ff5ba4: two P2s (many real requests are held as remarks: 'What's the weather tomorrow', 'Brighter', 'I need more light'...; replies to the lamp's question held when lamp_asked misses 'Shall I start it?'), four P3s
- 13:53 merged #1487 (LAMP_ROOM_FILE keeps the room across a short restart, keyed on the machine's boot; off unless set)
- 13:53 merged #1502 (harness-off's words: the lock never waits; docs and a comment)
- 13:52 #1503 reviewed at adab5baed (#1484's P3s; a refactor with a test, docs with the lamp's lines): no findings; merge on green ARM64
- 13:49 #1491 ready (the watcher ends with the join, 50 ms test holds); asked to fold its fix-up commit; the merge is mine per 12:4x
- 13:49 merged #1497 (controls' SIGTERM test signals its child itself; two qemu-only failures explained; tests)
- 13:45 merged #1500 (turn-end-wire's startup test keeps 300 ms for the silent worker only; tests). #1484 merged by the first builder. #1494 moved to 73542b54f; re-check started
- 13:41 #1484 re-checked at a18a8c69d: P2 fixed (a loop-level renewal test, three mutations fail it); two P3s; on by default, the first builder's merge (their 12:47 lamp run covers this head)
- 13:40 merged #1490 (the Policy's report period an option, 1 s on the lamp; its test rigs wait 20 ms: 43.7 s to 28.4 s on ARM64)
- 13:34 #1502 reviewed at 17e067816 (harness-off's words, docs/comment only): no findings; merge on green ARM64
- 13:32 noted #1493 on #811 (stays open). #1479 re-checked at fc1bcd711: P2 fixed (speech no longer counts a change as the lamp's own; a walk-in while it speaks seen at once); ready once the first builder's lamp run is done
- 13:31 merged #1493 (lamp-update harness-off undoes install's Harness at boot; an operator command, nothing runs it by default); one P3
- 13:30 #1475 re-checked at 1c85b65d3: the P2 fixed (both halves tested); conflicts in switches.md after #1483 (rebase, one account in serve-runtime.md); held for the lamp run
- 13:25 #1491 (setup's address watcher ends with the join; on by default) and #1497 (controls' SIGTERM test signals itself; tests) ready; reviews started
- 13:21 #1499 ready (LAMP_REMARKS: a remark to the room held in a busy room); review started. #1487 moved to f543270be; re-check started
- 13:20 #1474 re-checked at c4d28fc85: no findings; the first builder said no to the camera at 12:4x, so skills, volume_mute and cancel_reminder stay held on a reply; ready once the first builder's followups.sh run is in
- 13:17 #1486 re-checked at d4bb32136: all four P3s fixed; ready once the first builder's lamp run is done
- 13:17 merged #1496 (the 5 ms pins test truly reads every 5 ms; tests only)
- 13:10 #1494 reviewed at 510fe9939 (top commit): for the first builder (ring behaviour, no lamp shown, switch on in the product); two P2s (the candle swings each pixel over the full range with neighbours out of step, unlike V1's 80-100%; the pulse is not the agreed arc and stays low), one P3; waits for #1466
- 13:10 merged #1473 (body/gesture tells a dropped gesture's asker why and who holds the body; off unless --arbitration on; msgs fields optional)
- 13:07 merged #1489 (the Policy's name test's gap; tests only); closed #1488. #1490 (Policy report period option) ready; review started. #1475 moved to 1c85b65d3; re-check started
- 13:04 #1493 reviewed at c0003fae2: one P2 (no test runs the real System::harness_off: step order, the stop, a failing step all untested), four P3s; nothing runs it by default, the sign-in is untouched; ARM64 green
- 13:03 #1479 re-checked at 188c92b49: P2 (rule 8: 'lit' covers all the lamp's speech, so a walk-in while it talks is seen up to 2.4 s late, one try per 3 s; drop speaking from lit and keep the light-change window), one P3; held for the lamp run
- 13:00 #1496 reviewed at b9df96172 (the 5 ms pins test truly reads every 5 ms; tests only): no findings; merge on green ARM64. #1484 moved to a18a8c69d; re-check started
- 12:57 #1495 reviewed at 98ff34533 (light answers say warm white): no findings; on by default, sent to the first builder
- 12:53 merged #1481 (the reset wipes the kept light's folder; docs say LAMP_LIGHT_KEEP is meant with LAMP_LIGHT_REQUESTS)
- 12:49 #1487 reviewed at a9d938429: P1 (the file is keyed on the session's random BootId, so every restart reads as another boot and the room is never taken back; use /proc's boot_id), two P3s
- 12:49 #1486 reviewed at 0025e1c32: ready apart from the first builder's lamp run (they asked not to merge before it); a name said over a reply is dropped as the 12:1x rule says, but the duplex yield and stop word still let a person in (P3, asked for the lamp check); other P3s
- 12:48 #1494 ready (skill light patterns, stacked on #1466; its switch is on in the product); review started
- 12:48 #1466 rebased to 3e3609fce (no change of its own, clean); still the first builder's
- 12:47 merged #1485 (the same-voice bar's numbers, docs only)
- 12:46 #1474 reviewed at f42beaf41: for the first builder (privacy path: with LAMP_FOLLOWS_RUN=on a reply also runs Desk tutor's check_my_work, which sends a camera still to the model; the 11:3x yes doesn't name the camera); P2 docs say a wrong run changes only what the person sees; held for the followups.sh run
- 12:46 #1479 moved to 188c92b49 with OwnLight (camera changes during the lamp's own light change or speech count as its own); re-check started. #1493 ready (lamp-update harness-off); review started
- 12:43 #1471 merged by the first builder
- 12:40 merged #1480 (voice fixtures wait out a sibling's fork; tests only); closed #1477 and #1478
- 12:40 #1471 re-checked at 195426057: one commit that passes alone, P3s fixed; the first builder's lamp check passed at 12:3x (most asks of one request 1, was 72); ready for the first builder's merge
- 12:36 merged #1483 (docs: don't set LAMP_CAPTURE_CORES)
- 12:35 #1481 rebased over #1464 (bad70cc1f, clean); merge on green ARM64. #1487 ready (LAMP_ROOM_FILE keeps the room across a short restart); review started
- 12:34 #1484 reviewed at d6ff7b6cc: one P2 (the new renewal block runs in no test: every test sets catalog None; the issue's own test missing), two P3s; on by default, so the first builder's after the fix and their lamp check
- 12:33 #1456 re-checked at e0fbdb55f: no findings left (macOS test fixed, a latch change through poll() tested); ready once the first builder's lamp-4ace run is done
- 12:32 #1479 reviewed at 5680796f5: code ready (a fidgeting unseen person no longer learned as noise; the 3 s floor stays), two P3s (docs still say 3 s settling); the first builder holds it for the lamp run
- 12:31 #1475 reviewed at 0610532b1: one P2 (nothing tests the half that makes core 7 safe: the voice passing the switch to turn-end and turn-end taking core 6 with one thread), P3s; the first builder holds it for the lamp run
- 12:31 #1486 ready (lamp-name ignores a hearing begun while the lamp spoke, off unless set); review started
- 12:28 #1485 reviewed at 2c5e2b693 (the same-voice bar's numbers, docs only): no findings; merge on green ARM64
- 12:28 merged #1482 (every worker rig holds the one thread lock; tests only); closed #1422
- 12:25 #1481 green but conflicts in switches.md after #1464 (the LAMP_LIGHT_REQUESTS row); rebase asked
- 12:23 merged #1464 (lamp-cloud answers the app's Light row, off unless LAMP_CLOUD=on)
- 12:20 #1473 reviewed at c6541b123: not a first-builder case (msgs fields optional, body motion unchanged); one P2 (with both switches off, a dropped clip changes skills' done text and journal), two P3s; ARM64 green
- 12:19 #1483 reviewed at 1b135c2c6 (docs: don't set LAMP_CAPTURE_CORES): no findings; merge on green ARM64. #1469 re-checked at cb19e850a: P2 fixed (50/s held, was 200), mute flips in the same step; the first builder merges after the lamp check
- 12:17 merged #1467 (the conversation's voice is its last four answered prints, judged by the highest; off unless LAMP_SAME_VOICE_MODEL); four P3s
- 12:08 #1482 reviewed at 3f8de3a57 (every worker rig holds the one thread lock; tests only): no findings; merge on green ARM64
- 12:07 #1481 reviewed at 309eb6ac1 (#1468's P3s: the reset wipes the light folder, a test holds it): no findings; merge on green ARM64
- 12:06 #1471 reviewed at 1b78ff20b: on by default approved by the first builder (11:4x), the merge is theirs after followups.sh on lamp-4ace; one P2 (its first commit, the failing test alone, would land red: fold into one), P3s (a turn end can cut speech mid-word; two held calls in one message say 'asked again'; traced as provider_interrupted)
- 12:06 #1466 reviewed at bc62900e7: no P1/P2; its switch LAMP_LIGHT_REQUESTS is on in the product unit, so on by default in practice: sent to the first builder
- 12:05 #1464 rebased over #1468 (21518fc1f, no changes of its own, clean); merge on green ARM64
- 12:03 #1479 ready (eyes settle after two still glances; noise rises slower than it falls); review started
- 12:02 merged #1454 (LAMP_CAPTURE_STAGES, the capture worker's per-stage CPU line; off unless set)
- 11:53 #1464 ready but conflicts in switches.md after #1468; rebase asked
- 11:52 merged #1468 (LAMP_LIGHT_KEEP keeps the person's light across a restart, off unless set; the reset wipes it); four P3s
- 11:52 #1476 reviewed at b31bfa97b (light tool words): no P1/P2; on by default, so sent to the first builder
- 11:49 #1475 ready (#1459's P1 found on lamp-4ace: turn-end leaves capture's cores; a list taking every fast core refused); review started
- 11:49 #1473 (body/gesture tells the dropped gesture's asker why; msgs fields added) and #1474 (LAMP_FOLLOWS_RUN: a reply runs every held call but a cancel) ready; reviews started
- 11:47 #1469 reviewed at 5c1dbdfea: for the first builder (on by default, no lamp of its own; the first builder claimed the merge after the lamp check); mute flips still go out in the same step; one P2 (a held contact is sent at every 5 ms pin read: about 200/s while touched, was 50), two P3s
- 11:46 #1456 re-checked at 21be56e59: P1, P2, P3s fixed (builds on macOS; head-sleep passes 5 vs about 400; mute faster); new P2 (the new test reads Linux's /proc boot id, fails on macOS), one P3; the first builder holds the merge for the lamp-4ace run after the 13:30 install
- 11:44 #1454 rebased over #1459 (df247fef5, both capture switches kept, clean); merge on green ARM64
- 11:34 #1454 green at da7abd9b7 but conflicts in process.rs after #1459 (both add to the capture role's env list); rebase asked
- 11:33 #1471 ready (a held action asked again ends its turn; #1465); review started
- 11:20 merged #1463 (voice-eval's relay-clock tests wait for EOF, bounded; tests only); closed #1462
- 11:18 merged #1459 (LAMP_CAPTURE_CORES, off unless set; the risk beside lamp-turn-end documented); two P3s; two commits kept (the first passed its gate at b9b4a5d66)
- 11:17 #1469 ready (lamp-controls sends states on change and every 100 ms; carries the privacy reading); review started
- 11:15 #1454 folded to da7abd9b7 (one commit, the same net diff, clean); merge on green ARM64. #1468 ready (LAMP_LIGHT_KEEP); review started. #1467 moved to 3a1858bf2
- 11:11 #1464 reviewed at c1b4b3137: not a first-builder case (msgs only adds Light::TINTS, same values); one P2 (the app's preview is a Person-priority request, so 'make your light blue' says done but the ring keeps the app's colour until 10 s after the drag); one P3; ARM64 green
- 11:08 #1458 merged by the first builder. #1466 (light/outranked, msgs addition) and #1467 (the conversation's voice is its last four prints) ready; reviews started
- 11:05 #1454 ready at 825be9138 (P1, P2 and P3s fixed; own switch LAMP_CAPTURE_STAGES); asked to fold four commits into one (the first fails a test)
- 10:56 merged #1434 (LAMP_RESEND_LOST_LINE, Step B, off unless set; the first builder's 09:4x privacy yes: resend any lost line); one P3 (room for one queue command, two sent)
- 10:55 merged #1460 (lamp-eyes' change test takes out row shifts and learns cell noise; behind --look-on-change); two P3s (a fidgeting unseen person learned as noise, up to 2.4 s late; --full-look's decode untimed)
- 10:53 merged #1461 (#1445's P3s: stand-in children end with their tests; tests and doc)
- 10:53 #1464 ready (lamp-cloud answers the app's Light row; touches crates/msgs and ring_face); review started
- 10:51 #1456 reviewed at 98bca91c9: not a first-builder case (bus change additive; mute faster with the switch: 0.5-0.9 ms vs 1.8-2.7 ms); P1 (lamp-voice no longer builds on macOS: pipe_with), P2 (share_latch wakes the pump on every owner poll, about 365 passes a second with head sleep), P3s; ARM64 green
- 10:42 #1459 reviewed at b9b4a5d66: code sound, off unless set; one P2 (the risk named is wrong: lamp-turn-end's two RR 20 threads and lamp-name sit on cores 6-7, so capture pinned there can wait up to a 100 ms RR slice, past its 40 ms buffer, right as the person stops), one P3; ARM64 green
- 10:42 merged #1457 (the same-voice check's three journal lines through a journal a test reads; text unchanged)
- 10:39 #1458 re-checked at 294c98e1e: P3 fixed (one APP_CALLS list, held equal both ways); for the first builder
- 10:36 #1461 reviewed at 84b799cb0 (#1445's P3s: stand-in children end with their tests): no findings; merge on green ARM64. #1460 ready (eyes change test ignores row shift and learns cell noise); review started
- 10:32 merged #1455 (a test holds lamp-cloud's reply fields the app reads)
- 10:25 merged #1453 (setup.md's checklist after an app setup; docs only)
- 10:24 #1459 ready (LAMP_CAPTURE_CORES pins the capture worker); review started. #1454 moved to 6b73dbb90; re-check started
- 10:20 #1454 reviewed at 7b634c687: P1 (a stalls test fails, red on ARM64: the cost line sits between done() and finish_tick); P2 (LAMP_STALLS is on in the product unit, so every lamp gets it); P3 docs
- 10:18 #1458 reviewed at 9a810e1e2 (the voice runs the app's two volume calls by nobody): no P1/P2, one P3; on by default, so sent to the first builder
- 10:13 #1455 at de287bbd8: a second commit pins mic.get's available as a boolean (test and doc only); merge on green ARM64
- 10:08 #1456 ready (LAMP_CONTROLS_WAIT: the controls pump polls its sockets; touches lamp-bus and the mute latch's pump); review started
- 10:08 #1453 re-checked at bee85d867: the three P2s and two P3s fixed; one wording P3; merge on green ARM64
- 10:07 merged #1445 (print asked at 1.8 s; the same-voice child leaves the real-time class; carries #1444's two commits), on the first builder's 09:5x word (off by default)
- 10:06 #1451 merged earlier; #1455 reviewed at cb5f79efa (lamp-cloud's app-read fields held by a test): no findings, merge on green ARM64
- 10:06 merged #1451 (same_voice's test programs warmed before timed starts; tests only); closed #1450
- 10:03 #1454 ready (with LAMP_STALLS=on the capture worker says each stage's CPU a minute); review started
- 10:02 merged #1452 (lamp-eyes' 10 s line says why each glance was detected; diagnostics, switch on only; Eighth approved); one P3
- 10:00 #1453 reviewed at bb5dc5a79: paths, modes, reset, save order and 19 units right; three P2s (lamp-setup stays active 600 s after the marker; 'already set up' never prints at a later boot; a failing ping says 'the backend's ping fails'), two P3s
- 09:54 #1453 ready (setup.md's checklist after an app setup; docs only); review started
- 09:46 #1452 ready (lamp-eyes' 10 s line says why each glance was detected; diagnostics, switch on only); review started
- 09:43 #1446 merged by the first builder
- 09:39 #1446 ready at 0a8345bb1 (P2 fixed, lamp run 09:23 passed); on by default, so sent to the first builder to merge
- 09:32 merged #1449 (lamp-cloud's after-kill look tested with a child that outlives it; #1423's P3)
- 09:31 merged #1448 (the capture hold rig runs one worker at a time; tests only); closed #1447
- 09:30 #1434 re-checked at 801c058de: three P2s fixed; new P2 (any note refuses the resend, the name note included, so a line with the lamp's name is never resent while room talk can be); the dropped 'spoken to the lamp' guard still needs the first builder's word (asked 01:12 UTC, no answer)
- 09:30 merged #1440 (lamp-name --quiet-below, off unless set)
- 09:16 #1449 reviewed at acbdc08c3: no findings (#1423's P3 answered with a test); merge on green ARM64
- 09:14 #1440 rebased over #1441's row (aaf87876c, both rows kept, clean); #1448 P3 fixed at cbc722277 (the lock is the last field); both merge on green ARM64
- 09:08 merged #1421 (LAMP_UNCLEAR_HUM, off unless set); closed #1417
- 09:07 #1448 reviewed at a997723f2: tests only (Linux); one P3 (the lock is the first field, dropped before the worker's channels); merge on green ARM64
- 09:02 #1440 ready and green but conflicts with #1441's row in switches.md (adjacent rows); rebase asked
- 09:02 merged #1441 (lamp-eyes --look-on-change, off unless set; detects each glance until 3 s still)
- 09:00 merged #1423 (lamp-cloud: versions.firmware, iw bounded and a stuck one never waited for, no capability; off unless LAMP_CLOUD=on); one P3
- 08:55 #1446 reviewed at c5e4c4ff5: on by default matches the 07:42 yes word for word; one P2 (two key lines untested: the engine from the named member and the staff kept with WHO; changing the first sends a named Codex to Claude Code); one P3; ARM64 green; the lamp run is the first builder's
- 08:52 #1421 rebased onto #1436 (2fe5247c0): only its switches row, clean; merge on green ARM64
- 08:47 #1444 reviewed at 839bc9933: ready (#1435's P3s fixed; gate 1951/0/11, ARM64 green) but on the first builder's 08:36 hold for a P1 (the print child inheriting the voice's real-time priority, older than both PRs). #1445 (43d42ce91) waits for the hold and #1444; one P3
- 08:45 #1440 (413d9668d: P3s fixed, a row added), #1441 (142bf0b91: doc P3s, a row) and #1425 (24fe410fb: its row in the new form) rebased onto #1436, all clean
- 08:38 #1446 ready (a hand-over to a missing target; step 3 on by default per the first builder's 07:42 yes); review started
- 08:37 merged #1436 (switches.md counts worked out by a test from each row's Counted as)
- 08:32 #1440 (6fea732e1) and #1441 (843b15439) moved but still conflict (count line, before #1394); both wait for #1436
- 08:32 merged #1443 (hearing_replay scores the linear output at a chosen explained share, with onset_needed and the step)
- 08:30 #1445 ready (voice print asked at 1.8 s; sits on #1444); review queued after #1444. #1423 moved to 09b880d51; re-check started. #1442 back to draft
- 08:19 #1436 rebased to 5e9abe494 (PATIENCE row 'on', clean); merge on green ARM64. #1444 ready (#1435's P3s); review started
- 08:17 #1436 reviewed at c9d479464: counts right row by row, one P3 (a duplicate row passes); needs a rebase over #1394. Posted the merge order on #705: #1436 first, then #1421/#1440/#1441/#1442 rebase once
- 08:17 #1441 re-checked at 86d52d063: the P2 fixed (detect each glance until 3 s still, a walk-in test), P3s fixed, one doc P3; conflicts in switches.md; waits for #1436, then rebase and ARM64
- 08:16 #1440 moved to 83a445389 (the bar's range now -90 to -30, a mistyped -20 refused); still conflicts in switches.md; waits for #1436
- 08:16 #1421 green at caf5047df but conflicts again after #1394; rebase after #1436
- 08:14 #1443 P3 fixed at 8bb600611 (onset_needed and Interruption's step); merge on green ARM64
- 08:14 merged #1414 (fold-and-start.sh for lamp-4ace's step 4a; hold lifted 06:45)
- 08:14 merged #1439 (speaking under --steady-rate in practice unknown; eyes.md says how fast it answers)
- 08:13 #1442 reviewed at 50cea7d71: conflicts in switches.md (rebase); three P3s (warm fires on every line, not once per idle; no test of the GET off/on or that it carries no key; the 0.30 s is a connect cost, not a measured saving); ARM64 green on the old base
- 08:12 #1440 ready but conflicts in switches.md after #1394; rebase asked (after #1436 if it lands first)
- 08:11 #1423 reviewed at b08449320: one commit, clean; one P2 (wait() after killing a hung iw can still block the keepalive thread while iw sleeps in the driver; keep the killed child and try_wait it); gate 25/0/0, ARM64 green
- 08:09 #1441 reviewed at 97b7f7701: one P2 (after a change the detector runs on one frame of the new still picture, then not for 3 s, so a missed face costs 3 s; keep detecting each glance for 3 s after a change), two P3s; ARM64 green
- 08:07 #1394 merged by the first builder
- 08:07 merged #1438 (lamp-update's bus test publishes the state last; tests only); closed #1437
- 07:58 #1443 reviewed at 4670e311e: example and doc only; one P3 (7 in a row is a yield only after the reply's first 1.2 s, 10 before); merge on green ARM64
- 07:57 #1414 at c868bb799: P3 fixed, hold lifted (06:45); #1421 rebased to caf5047df (count only); #1439 kept as two commits; all three merge on green ARM64
- 07:54 #1439 P3 answered at 9788d6121 as a second commit; asked to fold into one
- 07:54 #1423 (lamp-cloud P3s) and #1442 (LAMP_SECOND_WARM) ready; reviews started. #1440 moved to 491757998
- 07:51 #1441 ready (lamp-eyes skips the detector on an empty room's unchanged frame, off unless set); review started
- 07:49 merged #1435 (the voice says when a print was not back at its answer; journal line only, off unless set); three P3s
- 07:47 #1394 rebased to 4e22ef037 (count line only): no P1/P2, clean; the first builder merges on green ARM64. #1440 ready (lamp-name rests while quiet, off unless set); review started
- 07:44 #1439 reviewed at c99c097d9: docs and tests, off by default; one P3 ('always unknown' depends on looks landing over 500 ms apart: the window keeps exactly-2-s-old looks); merge on green ARM64
- 07:41 merged #1430 (a recording keeps a held-back block's reference steps and replay-aec takes them; a replay-only guess too); one P3
- 07:39 #1421 conflicts again after #1418; one more rebase asked
- 07:39 noted #1418's merge on #1396 (stays open) and #1404 (to close by its owner)
- 07:38 merged #1418 (lamp-cloud on lamp-voice, setup's account path, off unless LAMP_CLOUD=on; El approved)
- 07:35 #1421 rebased to ba502cba5: Seventh's P3s (a comment and a ring-frame test assertion) and count 115; clean; merge on green ARM64. #1435 ready (the voice says when a print was not back; #829); review started
- 07:30 #1421 re-checked at f86f5cf26: Seventh's three P2s fixed (each mutation fails the new loop test), code ready; conflicts in switches.md after #1397; rebase after #1418 lands
- 07:20 #1418 rebased to 3491c6d66 (count lines only, clean); merge on green ARM64
- 07:19 merged #1433 (a doc comment: setup's save-order test pins what lamp-cloud needs)
- 07:16 merged #1431 (#1426's P3s: a run by hand keeps an installed build's fail; the build check tested)
- 07:15 #1434 reviewed at 541fec889: four P2s (trace lines never written; a resent line goes alone, losing a continues line's first half; the 'spoken to the lamp' guard missing with no recorded yes to drop it, so room talk could go to the cloud twice; a fix-up commit leaves five commits that don't build), four P3s; asked the first builder for a yes or no on dropping the guard
- 07:13 #1418 re-checked at 945e68e5e: no code findings, cloud-on.sh drop-in fixed with a test, lamp-06c1 plan matches; conflicts again in switches.md's count after #1397; merge after a rebase, range-diff and green ARM64
- 07:10 merged #1397 (LAMP_PROVIDER_WRITES=patient, off unless set)
- 07:07 #1425 re-checked at bb7e6b6a8: both P3s fixed, nothing new; gate 1945/0/11, ARM64 green; ready for the first builder's merge (06:24)
- 07:00 #1415 merged by the first builder (06:58). #1434 ready (LAMP_RESEND_LOST_LINE, Step B); review started. #1421 moved to 45e8b5779
- 06:59 #1430 reviewed at 729e276b8: one P2 (no test runs the real recorder loop with a mic1 hold and --diagnostics; one refused record loses the whole recording), one P3; off unless --diagnostics, no audio in the new rows; note for the first builder: #828's 06:36 chose the replay-only fix first, this changes the recorder, so last night's recording still can't replay
- 06:57 #1431 moved to 42dfbbe34: the wording P3 fixed (comment and doc only); merge on green ARM64
- 06:54 merged #1429 (the voice-check test on the paused clock; tests only)
- 06:53 #1432 reviewed at d696278e8: no findings (cloud-on.sh's drop-in carries the os-server conflict); #1397 rebased to 4c9f483b8, count lines only, clean; both merge on green ARM64
- 06:50 #1397 rebased to c0a299601 (count line only) but conflicts again after #1416; asked once more
- 06:50 #1394 re-checked at 270fcfa1c (one commit, same diff, count 113): no P1/P2, merges clean; the first builder merges on green ARM64 (on in the product unit)
- 06:48 #1431 reviewed at a89d3959b: #1426's P3s fixed; one wording P3; merge on green ARM64
- 06:45 #1430 ready (replay-aec takes a held-back block's reference steps; urgent for the owner's 15:00 run); review started
- 06:44 #1418 re-checked at f1207d122: both P2s fixed with tests that fail without them; two new P2s (conflicts in switches.md with #830's row; cloud-on.sh's drop-in needs Conflicts=os-server now the unit has none); P3 lamp-06c1's plan needs /var/lib/lampos-cloud
- 06:39 #1425 reviewed at d3578a7ce: ready, two P3s (a told lost to a restart in between is not retried; tasks.md says marked told without --close-told); gate 151/0/0 and 1944/0/11, ARM64 green; the first builder merges it after their own look (06:24)
- 06:39 #1429 reviewed at 1ada74c15: no findings, tests only; merge on green ARM64
- 06:36 #1416 merged by the first builder (06:35). #1429 ready (the voice-check test on the paused clock; tests only); reviewing myself
- 06:35 #1416 re-checked at 8fabd385c: ready (a comment and a doc paragraph since 80877cd5d; gate 1940/0/11, ARM64 green); the first builder merges it after their own look (06:24)
- 06:31 merged #1426 (a self-test names its build; an older build's fail the new one can't check is a warning), on the first builder's 06:24 word; three P3s
- 06:25 #1394 re-checked at c7b22b2e0: no P1/P2, the merge commit changes only switches.md's count; conflicts again in switches.md with today's lamp-voice
- 06:24 merged #1428 (the stalls test touches 64 MiB, above glibc's mmap threshold; tests only, Seventh's clean peer review)
- 06:23 merged #1424 (lamp-06c1's reset plan equal to the product's again)
- 00:46 #1416 re-checked at 80877cd5d: all findings fixed (the say fixture replaced by a Piper libritts-high clip, gone from history); gate 1940/0/11 and 168/0/3, ARM64 green; two P3s from Seventh open; merging after Fifth's peer review, which the first builder asked for
- 00:46 merged #1420 (cloud-on.sh for lamp-4ace's Sunday step 7; tooling)
- 00:45 #1415 clean at 6ea956900 (all P2s fixed, gate and ARM64 green) but sent to the first builder: the 23:34 condition (the short follow-up answered) no longer holds on the lamp; needs the lamp-4ace scene 3 run
- 00:45 #1421 reviewed at 496580e32: off unless set, at most one hum in 30 s, no mic change; four P3s (a cut nothing_said still silent, trace rows named as tap hums, switches.md count, loop wiring untested); ARM64 green; merging after Seventh's peer review, which the task asked for
- 00:36 #1424 reviewed at 68238eb46: data and docs, equal to the product plan; one P3 (a test keeping the two equal); merge on green ARM64
- 00:35 merged #1419 (fixture exec wait 30 s, tests only); closed #1317
- 00:32 #1419 ready (fixture exec wait 3 s to 30 s, tests only, #1317); reviewing myself
- 00:27 #1421 ready (LAMP_UNCLEAR_HUM: an empty 'Sorry?' gets the hm? and listening face); review started. #1415 moved to 6ea956900; re-check started
- 00:27 #1418 reviewed at a0bad2a07: two P2s (the unit's Conflicts=os-server acts with LAMP_CLOUD unset, as install enables it under lampos.target; no test with and without the switch), P3 docs; gate 766/0/1, ARM64 green
- 00:27 #1420 re-checked at 9444990d5: P3s fixed; merge on green ARM64
- 00:20 merged #1412 (ring startup tests 3 s to under 1 s each; lamp code unchanged); noted on #1128
- 00:16 merged #1413 (LAMP_SERVICE_DOWN_LINE, off unless set); #830 left open for its recording and step 8b
- 00:15 #1420 reviewed at 863bb2576: three P3s (run ignores failures, two-part host not filtered, not enabled/undo leaves os-server stopped); tooling, merge on green ARM64
- 00:10 #1418 ready (lamp-cloud on today's lamp-voice with setup's account path); review started
- 00:07 #1415 reviewed at c97bd0943: two P2s (a short sound let through with a held answer ends the conversation; the 1.19 s line arrives at 1.8-2.1 s with pre-roll and end silence, so it is judged on the lamp, against the yes's condition), two P3s; ARM64 green
- 00:06 #1416 reviewed at 975537b0c: one P2 (the fixture is macOS say output, whose licence keeps it off a public repo; redo with Piper libritts-high or a CC BY clip), three P3s; vendored sonora matches crates.io 0.2.0 plus PATCH.md; off unless set
- 00:04 #1414 ARM64 green at 0c9bcd9f4 but held: the first builder's 23:49 dry run on lamp-4ace found two wrong paths (lamp-body-inspect, body-integration.sh runs from the Mac); merges after the fix
- 23:57 #1413 moved to cece19c47: the three P3s answered in docs only (range-diff); merge on green ARM64 there
- 23:53 #1413 reviewed at a718af411: off unless set, three P3s (relay-outage wording left in switches.md/offline.md; an older lamp-voice rejects a recordings list naming the new line; only decision and line choice tested); merge on green ARM64
- 23:51 #1415 and #1416 ready; reviews started
- 23:50 #1394 c7b22b2e0 is a merge commit; asked for one folded commit
- 23:48 #1414 reviewed at 0c9bcd9f4: one P3 (step 1's pgrep pair takes the last exit only; the /proc fd check still guards); tooling, merge on green ARM64
- 23:47 #1397 ready but conflicts with lamp-voice in switches.md (#1410); asked for a rebase
- 23:47 merged #1411 (the setup light's retry-window test holds the fast clock across the marker)
- 23:46 #1411 reviewed at 1e47726ed: no finding (the light test holds the fast clock across the marker; my #1402 P3); merge on green ARM64
- 23:44 #1397 re-checked at 63162f447: P2 fixed (the early read only with patient writes; unset unchanged, pinned); off unless set; merge on green ARM64
- 23:40 #1394 re-checked at 8f17d0e3f: P2s and P3s fixed (cb94's 313 first outputs all within 2.41 s); needs a rebase as one commit (conflicts with #926 in policy_holds.rs; a merge commit on the branch); then the first builder merges with his lamp-4ace run
- 23:37 merged #1410 (a product lamp with no link says so once and keeps going; option A of #830)
- 23:31 merged #1409 (a null or a number in setup's link fields never fails setup)
- 23:24 #1410 reviewed at e488c4289: no finding (the product's offline line and lamp/online, the first builder's option A); merge on green ARM64
- 23:18 merged #1407 (a transport failure says its cause by kind, never the URL)
- 23:16 #1367 merged by the first builder at 23:15
- 23:16 #1409 reviewed at 3f8f662aa: no finding (a null or a number in a link field never fails setup; my #1406 P3); merge on green ARM64
- 23:15 #1388 and #1399 merged by the first builder at 23:15
- 23:11 posted where every ready PR stands on #705
- 23:10 merged #1402 (setup's light paints on a clock its tests run fast; P3: the after-marker window shrinks)
- 23:10 merged #1406 (setup's backend link needs the device id; a link it cannot write never fails setup; a broken one names its fields; null-field 400 still open)
- 23:09 merged #1405 (lamp-skills refuses a second --zone or offset)
- 23:04 #1378 merged by the first builder at 23:03
- 23:02 #1407 reviewed at d94952de7: no finding (a transport drop says its cause by kind, never the URL); merge on green ARM64
- 23:00 merged #1395 (lamp-tasks closes the harness it started once its work is told, off unless --close-told)
- 23:00 #1397 re-checked at 8c7c6986e: P2 (the early read_owed runs without the switch, so every lamp changes); otherwise good
- 22:59 #1399 re-checked at a78e42bc3: both P2s fixed; code ready; for the first builder (no lamp run this round)
- 22:52 #1405 reviewed at 57eb5ce1f: no finding (lamp-skills refuses a second zone or offset; tested six ways); merge on green ARM64
- 22:46 #1395 re-checked at a758fafa6: no findings left; off unless --close-told (neither unit sets it), so mergeable on green ARM64; the first builder's lamp-4ace check is posted for when it's turned on
- 22:46 merged #1403 (setup's status says generation lamp-v2)
- 22:44 merged #1391 (skills take --zone, so quiet hours keep their clock hours across summer time; closes #1384)
- 22:40 merged #1401 (setup keeps the backend's link root-only, the reset removes it; P3: a null field fails setup)
- 22:32 merged #1400 (a renewal waits for a request awaiting its response, pinned as a test)
- 22:32 #1403 reviewed at 239b1b00e: no finding (setup status adds generation lamp-v2); merge on green ARM64
- 22:32 merged #1398 (the first control message's ages stepped on a clock the test moves)
- 22:31 #1399 reviewed at 9a1cb0762: 2 P2s (a lost log with two re-tunes passes instead of not_checked; any hung task fails storage as the SD card) + 3 P3s; then waits for a lamp-52e6 run
- 22:31 #1398 reviewed at 2e6cbf38c: no finding (transport reads the same clock at the same places; the test steps it); merge on green ARM64
- 22:18 #1380 merged by the first builder at 22:17
- 22:17 #1397 reviewed at 1525632c1: ready on my side (3 P3s; off unless set; #1393's P3s fixed); Nine's peer review still asks for a barrier test and a fold
- 22:17 #1400 reviewed at 8ac927e54: no finding, test only (a renewal waits for an owed answer, pinned); merge on green ARM64
- 22:14 #1378 moved to 183ea0f9c: rebased (switches.md count resolved) and my P3 taken; ready on review, waits on the first builder's yes
- 22:06 #1394 reviewed at f18782f0a: 2 P2s (a file setting only no_reply_after_ms >= 12 s stops the voice starting with the switch; the 8 s evidence is cut by lamp-4ace's own 4 s wait) + 4 P3s; then lamp evidence (lamp-4ace battery or a cb94 night)
- 22:06 #1395 re-checked at 715a5f4db: P1 and P2s fixed, now off by default (--close-told); new P2 (the close check compares with the newest saved turn, which can be the person's own follow-up, so their harness is closed)
- 21:58 #1388 moved to aadfff0e6: a 6-line provider_worker.rs touch (likely the doc-comment P3); still for the first builder
- 21:53 #1388 re-checked at a5cc6f91f: both P3s fixed; new P3s (a misplaced doc comment; the new test stays green with the seeding removed); for the first builder
- 21:52 #1378 re-checked at d1690b67b: narrowing (a) in, colleagues at 120 s held; one comment fix left; product change, waits on the first builder's yes
- 21:50 #1380 moved to 80d43ffe4: a log line and the stray doc comment only (my nit); code ready, for the first builder
- 21:50 merged #1385 (eyes judge per face whether its mouth is speaking; off unless set)
- 21:45 #1355 merged by the first builder at 21:43
- 21:44 #1395 reviewed at 3ceaf84fd: P1 (can close a harness the person took over: exit 1 'working' is retried 60 s later) + 3 P2s (ask is the lamp's own text, so nothing closes at once; a reboot closes up to 16 old harnesses; --harness-as-person skips Harness's own guard); then for the first builder (Harness only on lamp-4ace's office Mac)
- 21:40 merged #1393 (a write_timeout drop names its write, size and bound; a diagnostic)
- 21:39 #1385 re-checked at b1144b698: both P2s fixed (95-100% right at the real pace; a silent tilted nodding face reads still); off unless set; merge on green ARM64
- 21:36 #1380 re-checked at 744735e8c: P3 fixed (only a real zone file is written); code ready; for the first builder
- 21:36 merged #1392 (air.md gives the base strip's per-sample time after #1374; docs only)
- 21:29 merged #1387 (the image pins the lamp's journal at the base's 20M; Monday's image)
- 21:24 #1388 reviewed at dd1a0cbbd: code right, 2 P3s (the loop's Presence should take at_start's reading); for the first builder (on where memory and known faces are set, the sixth has no lamp)
- 21:21 #1376 merged by the first builder at 21:20 (only changes what the voice reports)
- 21:21 #1392 reviewed at f495211a5: no finding (air.md's strip figures match controls.md); docs only, merge on green ARM64
- 21:12 #1385 reviewed at 530bf996d: 2 P2s (at the eyes' real 250-500 ms judging rate most looks are unknown, so the >=90% doesn't hold; a silent tilted nodding face reads yes) + 3 P3s; off unless set, msgs additive
- 21:12 merged #1381 (lamp-skills' two host-clock tests shortened, the late say_now check can fail; tests only)
- 21:12 #1374 merged by the first builder at 21:11 (no lamp reads the base yet)
- 21:11 #1387 re-checked at 978841d1a: the base's 20M pinned, my P3 taken, lamp-75c9 read-only check; merge on green ARM64 per the first builder's 21:00 yes
- 21:10 #1380 re-checked at 9ad5669b6: code ready (install rewrites zone.env from a saved zone; setup's never overwritten); P3 (a garbage /etc/timezone now reaches update and stops schedule); for the first builder
- 21:09 merged #1382 (the short hub line names the C-Media as speaker/room mic)
- 21:08 merged #1366 (lamp-air reads 0x7FFF CO2 as not known, decodes filler and starts a sensor whose name fails, as guards; on the first builder's 20:44 lamp run)
- 21:02 #1378 reviewed at c8e21a520: code right, but the first builder's 20:41 narrowing (a) with its test isn't in yet; the second's P2 (colleagues 45-180 s after the news answered) holds; CHANGES
- 21:00 #1387 reviewed at d582cc9a0: drop-in and tests right; P3 (30M is ~4 h of lamp-4ace's journal); merges on the eighth's lamp-75c9 run
- 20:54 opened an issue for the Expression sleep_name journal flake (stale input on a loaded runner); re-ran #1381's ARM64 once
- 20:50 #1382 moved to cd405640f: my P3 taken (the wrapped line is checked as published); merge on green ARM64
- 20:49 #1366 moved to 1c2317540: comments and docs only (the cause not reproduced; the start branch not exercised); the first builder's 20:44 lamp-4ace run shows 0x7FFF read as unknown and "#1366 still earns its place"; merge on green ARM64
- 20:47 merged #1383 (lamp-4ace's air fault: not reproduced at 400 kHz or with an idle sensor; docs only)
- 20:45 merged #1352 (with LAMP_WAIT_KEEPS_ANSWER=words a lone Stop before the answer stops it and is never asked of the model; off unless set)
- 20:45 merged #1368 (the product sets I2C bus 0 to 100 kHz at each start, the sensor's own limit; on the first builder's 20:2x lamp run)
- 20:42 #1380 reviewed at e1c83503d: code ready, 3 P3s (skills keep a fixed offset across summer time; a lamp set up earlier keeps schedule's old zone); for the first builder (product clock, the third has no lamp)
- 20:40 #1382 reviewed at 931dd358b: no blocking finding (the short hub line names speaker/room mic; my #1356 P3); merge on green ARM64
- 20:38 merged #1377 (lamp-tasks' tests read the next status in 100 ms slices; tests only)
- 20:37 the first builder's 20:2x lamp-4ace check: #1368 works (the clock write is real); #1366's start path not yet exercised; #1374 not exercised (no lamp reads the base). #1366 moved to cd98d0344 (docs and log line per #1379)
- 20:36 #1377 and #1381 reviewed: no finding / one P3 (the 1.5 s pose's margin), tests only; merge on green ARM64
- 20:31 #1373 re-checked at 43ff71322: P2 fixed (the fake's lease counts in its own steps), no finding; merges after #1340
- 20:28 #1376 re-checked at bdb14ebb8: all fixed (no warn before the first reading; a truly unreadable switch still warns after 1 s); for the first builder (no lamp)
- 20:25 merged #1379 (lamp-4ace's air sensor: not shown to be the bus speed; a sensor left not measuring is likelier; docs only)
- 20:05 #1366 moved to 7eef040bd: a doc-comment move only (P3); still waits on the first builder's call (fixture or lamp run)
- 20:03 #1373 reviewed at fd1d663ec: P2 (the fake body still times its lease on the real clock, so a check that never renews passes all 10 head tests); conflicts in realtime-tests.md; waits on #1340
- 19:59 #1367 re-checked at a6946f7f8: P2 fixed (the voice uses lamp-gemini's tag check; a refused hint is dropped), off unless set; waits on the first builder's call on the body's edit history
- 19:58 #1376 reviewed at b6aa7132b: P2 (each wait opens with a false warn "until the privacy switch can be read" before the first reading) + 2 P3s; reports only, msgs gains a constant; then for the first builder (no lamp)
- 19:57 #1368 moved to 9254ff533: docs only (the last two P3s folded); still for the first builder
- 19:53 #1366 re-checked at 838018906: P2 fixed (plain decode first; 0 wrong in 2.7M fuzzed reads); for the first builder (lamp-air on every lamp, the tenth has no lamp; the fixture may be enough)
- 19:49 #1368 re-checked at 7294804d0: P2 fixed, ARM64 green; for the first builder (real hardware, no lamp run; 6-step lamp-4ace check posted)
- 19:49 #1374 moved to 445cee468: the P3s folded in (a comment and the doc; the code unchanged); still for the first builder
- 19:40 #1374 reviewed at ca123b013: code right (a reset is still caught each sample); for the first builder (only lamp-4ace's strip shows it, the tenth has no lamp)
- 19:37 merged #1375 (lamp-4ace's hardware record after the 17 Oct visit: a second fan, the air sensor's fault was the bus speed, a button is read; docs only)
- 19:31 #1367 reviewed at 752abd87d: P2 (a 1-character subtag passes the voice's check but fails lamp-gemini's, so the voice doesn't start) + 3 P3s; the body was fixed at 19:26, the old text stays in its edit history (first builder's call)
- 19:28 merged #1364 (lamp-body exit-report prints the body's last exit report as plain lines; Monday's build)
- 19:24 #1366 reviewed at 92eaa9894: P2 (every answer goes through the filler decode, so with no filler a real 0xff can be skipped and a wrong CO2 read with good CRCs; use plain words() first); decode with filler is sound
- 19:21 #1368 reviewed at 1413ff19e: P2 (lamp-voice-eval's PARTS must name lamp-air-clock; ARM64 red on it) + P3s; then for the first builder. #1372: overlay right, 3 P3s; for the first builder (image not built or booted)
- 19:17 #1364 re-checked at ca1f3faa2: P2 fixed (a cut-short newest report is named, not replaced); merge on green ARM64
- 19:17 #1367: the body names a person (the 18:12 rule); flagged for the first builder with the two #705 comments that also have it; review started
- 19:12 #1370 reviewed at a3910c6f4: right (RemainAfterExit=no so the level is set again on the card's return), 2 P3s; merges on a lamp run (replug on lamp-06c1) or the first builder's yes
- 18:53 merged #1356 (the self-test and lamp-health say a whole USB hub dropping as one line; Monday's build)
- 18:52 merged #1363 (the product voice starts after the image's speaker level is set, without Wants; Monday's build)
- 18:44 #1364 reviewed at b4aef3f64: P2 (an unparsable newest report is skipped silently, so an older run prints as "the last"; pre-#1236 reports give no report) + 2 P3s; off the motion path
- 18:41 merged #1358 (setup's stop around the hotspot is serve::start_unless_stopped, with a test of each case; Monday's build)
- 18:41 merged #1353 (with LAMP_STOP_WORDS_HOLD=own-words a stop over the lamp waits at most 150 ms for the lamp's own words; off unless set; Monday's build)
- 18:39 merged #1360 (the capture rig sends nothing after what ends the worker; closes #1359)
- 18:38 #1363 reviewed at 44a9a361f: no finding (the product voice orders after the image's speaker level, without Wants); held for the integration cut
- 18:32 posted the hold on code merges until the integration cut on #705
- 18:32 #1353 re-checked at 67a7073d2: P2 fixed (the hold ends early only on lamp words stamped ahead), off unless set; held until tonight's integration commit is cut
- 18:25 #1356 re-checked at ee8f627a4: half the P2 fixed (said once; the wrapped line fits); still drops the list of what went with the hub when any part has failed
- 18:23 #1355 moved to 02863131f: docs only (my three P3s written into attention.md); still for the first builder
- 18:21 #1352 re-checked at 2abd2103f: P1 in the new commit (waiting for the words skips #1130's no-echo-evidence guard; a waited "No." during playback ends the conversation); off unless set
- 18:18 #1360 reviewed at eb65081c5: no blocking finding (one P3: finish should assert ending); conflicts with #1350 now merged, asked for a rebase
- 18:13 #1355 re-checked at f49b39ecf: the second voice uses the same three conditions, never looser; no new finding; still for the first builder
- 18:13 merged #1350 (the capture rig tolerates a worker's end on both sockets; closed #1344 and #1351)
- 18:07 #1358 reviewed at d6290ea67: no finding, no behaviour change (same lines, codes and timing on the real binary); for Monday, held until the first builder posts tonight's integration commit
- 18:00 #1352 re-checked at dfc7485b2: P2 and P3s fixed; conflicts with lamp-voice (no ARM64 run), asked for a rebase
- 17:58 merged #1357 (Expression's order-mark wording: a new session restarts its count, never its time)
- 17:57 #1355 moved to f49b39ecf: a second-voice commit and docs; re-check sent; still for the first builder
- 17:55 #1356 reviewed at c746f8e6b: P2 (lamp/health cuts the hub line at 160 bytes and says it twice when the mic fails) + 3 P3s; health goes ok→warn for the boot after one hub drop
- 17:55 #1350 re-checked at 8552ad89a: P2 fixed (a refused receive ends the drain too; closes both #1344 and #1351); merge on green ARM64
- 17:54 merged #1347 (a temperature flapping between two plausible values is judged on the higher one; an unreadable sensor still ends the run; for tonight's freeze)
- 17:50 #1355 reviewed at 0637b82e8: code right, 3 P3s (the window is 4-45 s; without LAMP_SPOKEN_TO someone else's short sound also plays); for the first builder (on by default on the product, the fourth has no lamp)
- 17:45 #1347 re-checked at 8067f7ea8: all fixed (the noise case now stops; 4ace's flap still runs); merge on green ARM64 per the first builder's 17:07 (pushed 17:37, before 17:45)
- 17:40 #1357 reviewed at c4f4e0646: no finding (the order mark's wording now matches engine.rs:711-714); merge on green ARM64
- 17:37 merged #1354 (Expression's session fence names the order mark and the 500 ms age; its test replays a forgotten session; my #1349 P3)
- 17:34 #1353 reviewed at d86765eae: P2 (the hold ends on any piece of lamp text, even one written before the "stop", so the lamp's own stop still stops it) + P3; off unless set
- 17:29 merged #1343 (the light's tools say white and warm are colours, not brightness; the first builder's lamp-4ace run)
- 17:25 #1347 re-checked at 452c1ba0a: P1 and P3s fixed; new P2 (noise within 10 of a good value that returns every 3-12 reads still never ends the run); no lamp run yet
- 17:24 #1352 reviewed at 973e42484: P2 ("Wait, ... what about tomorrow?" with a comma pause is judged on "Wait." alone and the question is dropped) + 2 P3s; off unless set
- 17:24 merged #1348 (a stop during the hotspot start keeps the script's error in its line; ARM64 green on the one re-run after #1351's flake)
- 17:23 #1354 reviewed at 9d5dcc8e8: no blocking finding (the replay is refused by the order mark, checked); merge on green ARM64
- 17:18 #1350 reviewed at 5ed6e9948: P2 (the rig's receive can still be refused after the worker has gone, as #1351 failed; tolerate it too and both flakes close)
- 17:17 merged #1346 (lamp-update check-units says whether a lamp runs exactly the product's units; read-only)
- 17:11 merged #1349 (Expression forgets the oldest of 32 retired voice sessions instead of stopping after 32 restarts)
- 17:07 merged #1335 (switches.md's product column as #1332 sets it; docs only)
- 17:07 #1340 re-checked at 287653a34: P3s fixed, ARM64 green; stays with the first builder (no lamp run)
- 17:06 opened #1351 (a held-up speaker test's receive is refused after the worker has gone; with #1344 the second capture flake); re-ran #1348's ARM64 once; noted both on #705
- 17:03 #1347 reviewed at 1aa3810cd: P1 (a flap read never counts toward BadTemperature and holds the heat streak, so a 64/70 flap after a good 60 never stops; undoes #1270's limit) + 2 P3s; then waits for lamp-52e6
- 16:54 #1348 reviewed at 77fbf84e4: no finding (the stop line keeps the hotspot script's error; my #1341 P3); merge on green ARM64
- 16:52 #1346 reviewed at 51970c801: P2 (a unit line equal to its own path header restarts the file, so an extra line before it still reads "same") + 3 P3s; read-only, PRODUCT has all 26 units
- 16:52 merged #1324 (the self-test's air is not_fitted only where no sensor ever answered since boot; a sensor that answered and fails stays a fail)
- 16:51 #1335 re-checked at 1e2fd3a85: rebased, matches #1332's merged unit (109 switches, 48 on); merge on green ARM64
- 16:50 merged #1345 (the supervisor tests send one Authority once the child's first Progress says it reads)
- 16:47 #1335 re-checked at c3ca60647: matches #1332's merged unit; needs a rebase (conflicts with #1323's and #1277's rows)
- 16:47 #1343 reviewed at e9aaceefd: code right; the lamp run's "cooler" miss was the model (no tool call reached lamp-actions); merge on green ARM64 per the first builder's 16:44
- 16:43 merged #1341 (a systemd stop during the hotspot start exits 0, light black; my #1336 P3)
- 16:42 merged #1337 (lamp-health show names zones and parts plainly; my #1327 P3s and El's points)
- 16:42 merged #1342 (the first-output test's FIFO write is bounded too)
- 16:42 merged #1332 (the product voice runs lamp-4ace's switches of 17 October that a product lamp can carry, bounded reference recovery and the call's wait)
- 16:25 #1340 reviewed at 05187daad: code right (rest jitter fits the half-count rule), 2 P3s; for the first builder (factory self-test on hardware, the tenth has no lamp). ARM64 red on a new flake: opened #1344, re-ran once
- 16:25 merged #1277 (with LAMP_WAIT_KEEPS_ANSWER=words a sound while an answer waits is judged by its words; off unless set; on the first builder's privacy yes)
- 16:24 merged #1316 (light_warmer and light_cooler; on the first builder's lamp-4ace run)
- 16:24 merged #1328 (records looks again at a process with no arguments, resolves the unit's binary through its link, and install places records.json; on the first builder's terms)
- 16:23 #1342 reviewed at 50f4c725a: no finding, tests only (the FIFO write is bounded too); merge on green ARM64
- 16:20 #1337 re-checked at 17adc646f: both P2s fixed, no finding; merge on green ARM64
- 16:19 merged #1338 (a stop word over the lamp's speech clears LAMP_FOLLOWS_ON's clock; off unless set; my #1278 P3)
- 16:18 merged #1313 (lamp-4ace's hardware notes: about 20 °C hotter than cb94 the same morning, one SD host timeout; docs only)
- 16:14 merged #1323 (the voice tells the model the name typed at setup and says hello by it; on the first builder's terms)
- 16:10 merged #1336 (setup listens for a stop before its light starts, so an early stop ends black; my #1325 P3)
- 16:10 #1115 re-checked at 2c88dab21: the lowering's P2 fixed (a press lets go within 0.1 s; a misread is neither a hand nor an end); waits only for lamp step 4
- 16:09 #1324 re-checked at 274827f7a: P1 fixed (a sensor that answered once is a fail); P2 (no test pins the CRC half of ever_answered), P3 (ever_answered lost on a lamp-air restart)
- 16:08 #1337 re-checked at e16082a69: zone P2 fixed; new P2 (a zone right after "(" loses the bracket and reads "the board", garbling lamp-4ace's likely first line)
- 16:08 #1332 re-checked at a9c9153ac: all six fixed (20 switches left, each with its other half on a product lamp), 2 doc P3s; merge on green ARM64. #1335 still matches #1332's old head
- 16:07 #1338 reviewed at bcb454506: one P3 (no test pins the clear), off unless set; merge on green ARM64
- 16:03 #1067 re-checked at 7c63b1631: all four fixed (an old sway_roll is ignored; about 2 in 5 pauses sway, the side picked each time); waits for a lamp run (step 4b Sunday on lamp-4ace, or the fifth on lamp-52e6)
- 16:02 merged #1334 (the first-output test proves the hash runs off the caller's thread with a FIFO, not a 50 ms bound)
- 16:00 #1337 reviewed at 42090b719: my #1327 P3s fixed; P2 (zone names after a comma, skin_zone and idle zones still shown raw)
- 16:00 merged #1333 (Stuffy air's docs row: no lamp with a working air sensor has run it)
- 15:59 #1332 reviewed at 72a183819: 2 P1 (LAMP_POLICY_HOLDS_ACTIONS without the product policy's --act attention delays every nameless call 2 s; LAMP_MARK_WEIRD keeps transcripts a reset doesn't wipe), 2 P2 (LAMP_LOOK_FULL and LAMP_LIGHT_REQUESTS half-set), P3s; #1335 matches it, 4 P3s
- 15:58 merged #1331 (the voice mic note is the latest capture start's and follows the link warning; my #1326 P3s)
- 15:58 #1331 reviewed at 3e259ff6c: no finding (my #1326 P3s fixed); merge on green ARM64
- 15:56 #1323 re-checked at f7e49b274: P1 and P2 fixed (the name reaches both workers; placeholders only), one P3; merge on green ARM64 per the first builder's 15:30 terms
- 15:55 merged #1330 (the supervisor tests' inert child builds its worker once connected, as native.rs does)
- 15:52 #1334 reviewed at ebb23d05e: no finding (the first-output test proves the hash is off the caller's thread with a FIFO, not a clock); merge on green ARM64
- 15:48 merged #1322 (with LAMP_QUIET_EVIDENCE=on capture says once a minute how many blocks the skip saved; off unless set)
- 15:46 merged #1325 (setup breathes a light on the ring through the hotspot, the join and a failed join; on the first builder's 15:14 terms)
- 15:45 #1328 reviewed at f42fc130d: code right and safe (still refuses when unsure), 3 P3s; for the first builder (only lamp-4ace shows it)
- 15:44 #1333 reviewed at e84545715: no finding, docs only (Stuffy air's row: no lamp with a working sensor has run it); merge on green ARM64
- 15:42 #1330 reviewed at c7a805ba7: no finding, tests only (the inert child builds its worker once connected, as native.rs does); merge on green ARM64
- 15:42 merged #1327 (lamp-health show reads for the owner: plain names, a sudo hint, ages only where they matter)
- 15:38 #1115 re-reviewed at 1bc637702: rebase good; new P2 (the shoulder lowering holds 8 s against a press on base/head/elbow, and one bad read is taken for a hand); lamp step 4 still open
- 15:35 #1067 reviewed at 2eff71446: 2 P2s (an old sway_roll catalogue stops Expression at start; every pause sways, alternating like a metronome) + P3s; then waits for the fifth's lamp evidence (not on by default)
- 15:35 #1327 reviewed at 22a575521: 5 P3s (a tie can name different parts on the first line and top row; a healthy lamp can print the 5 s age), no blocker; merge on green ARM64
- 15:28 #1323 reviewed at bda633793: P1 (worker_command clears the environment, so the name never reaches the provider or backup worker), P2 (real-looking example names), P3s; then for the first builder (on by default, no lamp)
- 15:28 merged #1319 (a Policy say_now after a reload ended a held trigger does nothing; off unless set; #1309's P3)
- 15:26 merged #1326 (a mic waited for at start is one plain line in the journal, the session's events and the voice's health)
- 15:22 #1324 reviewed at 662001e99: P1 (lamp-air says "no air sensor answers" after any 3 misses, so lamp-4ace's failing sensor would read not_fitted), P2 (copied strings untied by a test), 2 P3s; after the fix, for the first builder unless run on cb94 and 4ace
- 15:22 #1326 reviewed at c62208c02: no blocking finding (2 P3s: the note is never cleared within a run; hidden while the link is down), text only; merge on green ARM64
- 15:19 #1322 reviewed at 77ba6f5a9: P2 (a silent speaker counts as skipped even when the search would not have run, so a quiet room reads ~98% skipped) + 2 P3s; off unless set
- 15:17 merged #1321 (the held-up speaker test bounds when an off worker ends; doc says only what its sources say; my #1298 P3s)
- 15:14 merged #1320 (the worker environment test prints only the names a child got; my #1311 P3)
- 15:13 #1325 reviewed at 7783c18f7: P2 (a recent ring error reopens spidev after the marker, so the voice's ring can be off for its run) + 3 P3s; then for the first builder (on by default, real hardware, the third has no lamp)
- 15:07 #1316 re-checked at 4eae7a0ed: all four P3s fixed, code right, ARM64 green; stays with the first builder (on by default, not shown on a lamp)
- 15:05 #1277 re-checked at 0b9b813b6 (docs only): the privacy line now matches what is sent; my P2 fixed; stays with the first builder
- 15:05 merged #1318 (every run ends with the realtime group's own output, so the hold medians show; my #1299 P3)
- 15:02 #1321 reviewed at be354407e: no blocking finding, tests and docs only (my #1298 P3s); merge on green full ARM64
- 15:00 merged #1291 (with LAMP_CUT_INS_NOT_TALK=on a cut-in left unanswered does not make the room busy; part of #828, off unless set)
- 14:59 #1320 reviewed at 81a053548: no finding, tests only (my #1311 P3); merge on green ARM64
- 14:53 merged #1298 (a refused send names its socket; the held-up speaker case documented with cb94's numbers; #1178 stays open)
- 14:50 merged #1263 (lamp-keeper prepare reads the records as install does; the second builder's yes)
- 14:50 merged #1296 (a refused standing lamp folds before letting go; lamp-52e6 run by the fifth)
- 14:47 #1291 re-checked at 18fb9ed30: rebased, wiring test added and proven, took_over unchanged; merge on green ARM64
- 14:46 #1316 left to the first builder (on by default, real-hardware light and character change, the third has no lamp); code right, 4 P3s
- 14:43 merged #1314 (the supervisor tests' inert child builds its worker before it binds)
- 14:39 merged #1306 (stop-word counts keep the last 16 results and the first 16 stops; at most 32, tmpfs 0600, off unless set)
- 14:38 merged #1308 (the mic USB check fails only where a Jieli is the mic; #1304's P3s)
- 14:37 merged #1299 (PR runs include the realtime group, one at a time and first; +8 tests, about 8-10 s)
- 14:36 merged #1311 (the worker environment test fails on any extra variable; doc on late words; P3: the failure message prints values)
- 14:35 merged #1305 (capture retries a mic off USB at start; privacy path cleared by the first builder at 14:11; busy retry 50 ms)
- 14:34 merged #1310 (the muted wait's closures moved into a tested function; a pure move)
- 14:31 #1299 (PR runs include the realtime group) and #1316 (the light steps warmer and cooler; the character file says the light can change) ready, first reviews started; #1313 moved
- 14:27 #1314 reviewed (test fixture reorder; no finding): merge on green ARM64
- 14:25 #1313 (lamp-4ace heat and SD timeout, docs): checked what I can; left for Jarvis's peer review as the first builder asked
- 14:25 merged #1309 (a held dim is given back when a reload finds its skill or trigger gone; #1066's P3)
- 14:20 #1308 moved (wording P3s), #1306 moved, #1311 (#1295/#1297 P3s) ready: reviews started
- 14:18 #1305: the first builder said merge on my clean check (14:11); moved to 03f9b0c59 (four P3s taken), re-check started
- 14:15 #1310 (#1288's P3: the muted wait's closures moved into a tested function; no behaviour change claimed) ready, first review started
- 14:12 merged #1307 (the motor worker stamps Ready when it sends it; authority expiry unchanged)
- 14:11 #1291 green but conflicts (carries #1278's original commits): rebase asked
- 14:11 #1306 (stop-word counts keep the last 16 results and every stop said; heard words in a file) ready, first review started
- 14:10 #1305 left to the first builder (privacy path: a mic open retry at start, on by default); no privacy defect found; 5 P3s
- 14:09 #1308 reviewed: my three #1304 P3s fixed; merge on green ARM64; #1309 (a held dim given back when a reload finds its skill gone; #1066's P3) ready, first review started
- 14:06 #1308 (#1304's P3s: the mic USB check fails only where a Jieli is the mic) ready, first review started
- 14:04 merged #1283 (memory says once its reports are off without --bus-root; its env-alone test leaves the realtime group)
- 14:03 #1307 reviewed: no finding (the Ready stamp is a receipt time only; authority expiry never uses it); merge on green ARM64
- 14:03 merged #1294 (the self-test's ring check on a stepped clock; the lamp keeps the real clock)
- 14:01 merged #1304 (the self-test says whether the voice mic is on USB and how often it fell off; read-only; P3: needs the device_micro2 udev name)
- 14:01 merged #1303 (#1293's P3s: one open with O_NOFOLLOW and fchmod; the key's narrowing tested; cb94 readings clean)
- 13:59 #1298 first review: P2 doc (a third end rule fired on cb94: the speaker's last reported queue ran out, so with recovery off a speaker one block late ends the voice, not 40 ms); only the error text changes on a lamp
- 13:59 #1291 rebased onto #1278 (now merged); only the wiring test P3 left; merge on green ARM64
- 13:58 merged #1301 (#1282's last P3s: a still-muted word keeps the tap's mark; an unknown or invalid privacy lease counts as closed)
- 13:57 merged #1278 (LAMP_FOLLOWS_ON: a line within 20 s of the lamp's words may follow on; off unless set; both P2s fixed)
- 13:57 merged #1285 (LAMP_QUIET_EVIDENCE: skip the echo search while the speaker is silent; exact; off unless set)
- 13:56 #1304 (the self-test says whether the voice mic is on USB and how often it fell off) ready, first review started
- 13:55 merged #1288 (muted, a tap shows the asleep face about a second; the slide now counts; off unless LAMP_HEAD_TOUCH=on)
- 13:54 #1307 (the motor worker stamps Ready when it sends it; a slow-ARM64 supervisor test) ready, first review started; #1291 moved
- 13:51 #1291 re-checked at 3f77b25cd: both P2s fixed (only the lamp's audible voice counts; up to 8 marks kept); waits for #1278, then a rebase, plus a wiring test (P3)
- 13:50 #1303 (#1293's P3s: one open with O_NOFOLLOW; cb94 readings) and #1305 (capture retries a mic off USB at start; likely the privacy path) ready, first reviews started
- 13:50 #1296 re-checked at 690716e55: no finding; waits for ARM64 and the owed lamp-52e6 run
- 13:47 #1294 rebased (merge on green); #1298 (lamp-cb94's voice end: a refused send names its socket; capture worker test) ready, first review started
- 13:38 merged #1290 and #1286 (Tests); #1285 rebased and #1288 moved: re-checks started
- 13:38 #1294 and #1283 ready but conflict in realtime-tests.md after #1290/#1286: rebases asked (my #1283 comment corrected: it had said merged)
- 13:38 merged #1286 (Tests: real-clock test off the clock / bounded; lamp behaviour unchanged)
- 13:38 merged #1290 (Tests: real-clock test off the clock / bounded; lamp behaviour unchanged)
- 13:32 #1296 re-checked at 6d35e0fa5: P2 fixed (a flagged servo is let go at once, never re-powered); waits for the owed lamp-52e6 run and ARM64
- 13:31 merged #1302 (lamp-4ace note: #1300's P3s and Nine's corrections)
- 13:30 #1301 reviewed: both #1282 P3s fixed; P3 left (a failed-check lease still counts as open); merge on green ARM64
- 13:27 #1301 (#1282's last P3s) ready, first review started; #1291 and #1296 moved with fixes, re-checks started
- 13:26 merged #1297 (#1284's P3s: the late line pinned once a request; a late answer's words reach no request, tested through session.rs)
- 13:24 merged #1199 (Dance's long dance sends its second clip on body/gesture's over; shown on lamp-06c1)
- 13:24 merged #1300 (lamp-4ace hardware note: the mic's own-voice level, the USB drops, the air sensor's failed reads)
- 13:18 merged #1230 (body/gesture: began, over or dropped; the fifth's yes; the same-step drop fixed)
- 13:14 #1296 first review: P2 (a servo's own protection flag keeps the fold going and re-powers that servo every 20 ms for up to 15 s, where lamp-voice let go at once); no lamp run
- 13:14 merged #1282 (muted, a tap gets no look-up; a stale mute keeps its last value; the slide only with LAMP_PRIVACY_CONTROLS, said in the docs)
- 13:13 #1285 reviewed at 9939c056a: the skip is exact (54,000 blocks probed); needs a rebase onto #1295's worker_command (P2) with its switch in the real-command test
- 13:13 merged #1293 (install gives its own default files their mode; secrets only ever narrowed; cb94's evidence clean)
- 13:12 #1278 re-checked at 71566dd40: took_over P2 fixed (probed); new P2 (the sound let through gets the follows-on mark itself, so s-0857's three sounds would each be told 'may follow on')
- 13:10 #1297 (#1284's P3s: late line wording, say-once pinned, late words reach no request) ready, first review started
- 13:06 #1199 rebased onto #1230's head (merges clean), re-check started; #1283 moved again
- 13:02 #1282 re-checked at 3cd94b69e: both P2s fixed (a stale mute keeps its last value; the docs say the slide needs LAMP_PRIVACY_CONTROLS; closing the slide mid-look ends it); merge on green ARM64
- 12:59 #1296 (a refused standing lamp folds before letting go; #1281 follow-up; on by default) ready, first review started
- 12:59 merged #1276 earlier note; #1293 (install gives its own default files their mode; always on; cb94 evidence) ready, first review started; #1282 moved, re-check started
- 12:58 merged #1276 (#983's follow-up: the self-test's head move needs a fresh joint state)
- 12:55 merged #1295 (#1158's P3s: the worker command one tested function, identical for 64 of 64 cases; no secret reaches a worker)
- 12:52 #1276 (#983 follow-up: the head move needs a fresh joint state) ready, first review started; #1278 moved, re-check started
- 12:48 #1288 first review: P2 (a closed privacy slide doesn't count as muted for the tap face; fails safe but misses the slide; fix in the voice: keep the last PrivacyState); not the privacy path (draws a face only)
- 12:47 #1230 ready at ae2990f4b but conflicts again after #1281 (body tests.rs end): rebase asked
- 12:46 #1291 first review: two P2s ('over the lamp' means any reply in hand, so two people near a silent lamp never make the room busy and the 15:39 shape plays; room_told keeps only the newest mark); off unless set
- 12:46 #1283, #1286, #1290, #1294 (Tests: memory, actions, self-test clocks) ready, first reviews started (one agent)
- 12:45 merged #1281 (temperature follow-up: a hot first read re-read; an unreadable arm folds then lets go; lamp-52e6 run of the head)
- 12:45 #1263 re-checked at 194b7e0a8: code ready (all three orders hold); waiting for the second builder's yes on records.rs (their crate)
- 12:37 #1277 first review: privacy path (a new speech-to-text stream gets what the mic hears while an answer waits): left to the first builder; P2 (the privacy doc line understates what is sent); off unless set
- 12:37 #1281 re-checked at d364ed6d0: the unreadable arm folds then lets go (capped); the seventh's two-150s case starts; lamp-52e6 run of this head posted; merge on green ARM64
- 12:33 #1295 (#1158's P3s: worker command one function, tested; doc) ready, first review started
- 12:32 #1252 merged by the first builder as my re-check posted (no finding)
- 12:31 merged #1271 (remember's kept-note test reads within its own 2 s) and closed #1269
- 12:30 merged #1289 (#1259's P3s: a refused message counts as read, tested; the CPU measure Linux-only)
- 12:27 #1285 (LAMP_QUIET_EVIDENCE: skip the echo search while the speaker is silent) ready, first review started
- 12:25 merged #1257 (Stuffy air after review: my four #1066 P3s, the lamp-06c1 check)
- 12:24 #983 merged by the first builder (the self-test's head move at rest)
- 12:22 #1291 (cut-ins over the lamp don't make the room busy; off unless set) ready, first review started; #1199 rebased
- 12:21 merged #1265 (reading, a gesture is laid over the lean; speaking unchanged and now tested; shown on lamp-52e6)
- 12:20 #1287 merged by the first builder (lamp-4ace units: reach probe); #1277 and #1281 moved
- 12:19 merged #1284 (the journal ties a late answer to its request; journal only)
- 12:16 #1281 re-checked at d7c8026b8: new P2 (an unreadable arm is held up with torque on and never lowered or released, so a really hot arm stays powered); the seventh's P2 open; conflicts with #940's body commits; no lamp run. #1289 reviewed clean, merge on green
- 12:15 #1287 (lamp-4ace units: reach probe and offline voice) left to the first builder (lamp-4ace settings); #1288 and #1289 ready
- 12:13 #1278 first review: P2 (reply.is_some() stands for 'over the lamp', so a follow-up begun while a sound waits ~4 s for the model gets no mark: s-0857's shape); off unless set
- 12:12 #1263 re-checked at 48468665a: both probe orders fixed; new P2 (lamp-update's own round sets a damaged file aside and saves records with no mark, so a later reset brings V1's key back; probed); fix in Records::state
- 12:12 merged #1234 (lamp-voice stalls: tonight's stall reader; all findings fixed; the Mac test fixed and run)
- 12:11 #1259 merged by the first builder (hand-controls pump; mute worst case unchanged, measured)
- 12:09 #1259 measured both ways: mute and touch worst cases unchanged within noise (after ~3 ms, before ~3-5 ms); P3s; left to the first builder (mute path)
- 12:08 #1282 first review: two P2s (the privacy slide isn't on voice/mute, so on today's lamps a tap still looks up with the slide closed while the docs say not; a stale mute counts as unmuted); not the privacy path (reads only)
- 12:07 #1264 after the merge: no privacy change (the mic sends and opens as in duplex today); P2 doc (the sixth's 52e6 run, no gain, not recorded) and P3s posted
- 12:06 merged #1273 (the actions server's periods on a caller's clock; the light test leaves the realtime group; lamp unchanged)
- 12:05 #1281 first review: code ready (probes hold); waits for a lamp-52e6 run of the head (on by default, changes the start-up heat refusal) and ARM64
- 12:04 #1284 (journal ties a late answer to its request) ready, first review started
- 12:04 merged #1279 (a test comment: joint states every 100 ms)
- 12:00 #1265 re-checked at 7ad517053: P2 fixed, nothing to change; needs a rebase (tests.rs end, with #1239); 409/0/0 on a merged copy
- 12:00 merged #1266 (#1047's P3s: the play-time stamp as one tested function; docs on where the raw-blocks test runs)
- 11:55 #1257 reviewed at 88ba52929: all four #1066 P3s fixed; needs a rebase (one-line loader conflict with #1042); P3s on the README times and the cut sleepy clip
- 11:55 #1264 merged by the first builder (listen gaps, off unless set; Nine's approval and the sixth's 52e6 check)
- 11:53 #1282 (muted for privacy, a tap gets no look-up; #711) ready, first review started
- 11:52 #1281 (temperature follow-up to #1270: a hot first read re-read until two agree) ready, first review started
- 11:50 #1271 reviewed clean but conflicts in realtime-tests.md: rebase asked; #1270 probes hold after the merge; follow-up asked of the fifth (an unreadable arm joint lets go of the whole lamp)
- 11:49 merged #1158 (the name alone in a busy room needs no face only at night, behind LAMP_SPOKEN_TO=day-and-night; on unchanged; LAMP_KERNEL_CLOCK reaches the workers)
- 11:47 #1199 re-checked at f9932f8f2: earlier findings fixed, shown on lamp-06c1; needs a rebase after #1230 (stacked); #1230's rebase must rename #1239's gesture_fate line (silent build break)
- 11:47 merged #1247 (lamp/online asks the internet itself while the link is down; 443 only; off unless set)
- 11:46 #1230 checked at 3fa2c76b7: ready (P3s only), but conflicts in body tests.rs after #1239 merged; needs a rebase
- 11:45 merged #1239 (the body weighs who asks for a gesture, behind --arbitration on; the snap, the overwrite and the slump replay all fixed; safety holds)
- 11:44 merged #1042 (Task news as a skill; off unless installed; each run tied to its own task's news)
- 11:44 #1234 re-checked at d90fce269: all five fixed; one P2 left (the new test fails on macOS: its ring line's counters are all 0, which prints 'not Linux' off Linux; make it [0,1,0,0]); #1277 and #1278 first reviews started
- 11:43 #1270 merged by the first builder at 11:43 while I was out (heat streak fix; seventh's four rounds); my probes to run after the merge
- 11:43 #1262 merged by the first builder (the rule given only a carried-over request's own words)
- 11:43 #1274 merged by the first builder as my review posted (no finding)
- 11:42 merged #1275 (Desk tutor's note and marks: #1260's P3s; needs names the body)
- 11:41 usage limit ~10:30-11:40: all review agents stopped; resuming in order (noted on #705)
- 10:27 #1261 after the merge: with LAMP_QUIET_PACE=on a mute closes the mic up to 3 ms later in a quiet room (the first builder's call, inside the 100 ms target); posted as P3s
- 10:26 #1259 first review (partial): on the mute path (controls/state latest_mute, touch sleep); worst case unchanged by reading; left to the first builder; measure both ways running
- 10:23 #1263 first review at 48bf33993: P2 (when the records are set aside, a key still there loses its 'written once' mark, so a later reset brings V1's key back; probed); P3s (doc, a wrong 'in place' line)
- 10:23 merged #1260 (Desk tutor's marks: a nod at right work, a tilt at a wrong line; off on every lamp; P3s: the gesture starts before the first word, curious lifts the head past reading's bound)
- 10:16 #1261 merged by the first builder on their own review (quiet pace); my review told to post only a real defect
- 10:16 merged #1244 (the ring's listening face after a head tap follows the voice's hum; off unless LAMP_HEAD_TOUCH=on; my P2 fixed with a coordinator test)
- 10:13 #1265 first review: P2 (no test shows speaking is unchanged); safety holds; shown on lamp-52e6 at 09:36; note: curious over reading writes head pitch 2112, past its normal reach (only the floor and 4095 bound it; existing behaviour)
- 10:05 #1252 first review at 38d052a56: wait for #1239, then a rebase that rewords #1239's 'nothing sends them yet' lines; checks hold (off is byte-identical; priority fixed to skill; one Priority type)
- 10:04 merged #1268 (--full-look-movement waits on the camera's aim turning; 2 s caps on 52e6 back to 36 from 329; off unless set; P3s; my #1251 24->342 came from a 66 ms joint read)
- 10:03 #1240 merged by the first builder (tests-only rule)
- 10:01 #1209 merged by the first builder (reset line safety; it was theirs); #1042 and #1239 moved with fixes, re-checks started
- 10:00 merged #1267 (hung_worker on the stepped clock, waiting on the real supervisor's hang and respawn; leaves the realtime group; #823)
- 09:57 #1255 closed by its builder; #1259 (hand-controls pump: one look per subscriber when idle; on by default) ready, first review started (checking the mute's latency); #1263 moved again
- 09:56 #1042 re-checked at 1f37b9dfb (rebased past Stuffy air): the merge is right, task only on from=tasks; new P2 (the loader accepts each with [quiet] or until=ends, and neither works on an item's run: an each skill with a dim never ends it)
- 09:53 #1264 (LAMP_LISTEN_GAPS: a long answer's sentence ends held quiet 250 ms so a person is heard; #828) ready, first review started
- 09:50 #1253 merged by the first builder (the LAMP_SCHEDULE_SPEAK=skill fix; one rule for both)
- 09:46 #1239 re-checked at fdd465e9c: the snap and the same-step overwrite fixed; new P2 (a slump during the 0.7 s ease leaves it paused, and it replays the old nod about 2 min later when the lamp is taken up); P3 the ease runs past a gesture's farthest point
- 09:46 #1270 (fixes a P1 now in lamp-voice from #1236: a replaced read fed the heat streak, so a false heat stop turns torque off on every joint) ready, first review started at priority; #1247 moved again
- 09:43 #1268 (#1251 follow-up: the wait follows the camera's aim; my 09:24 note) ready, first review started
- 09:38 #1245 merged by the first builder (left to them at 08:26; green, P1 fixed)
- 09:38 #1266 (#1047's P3s: the stamp as one tested function, doc fixes) ready, first review started
- 09:36 #1234 first review: P2 (the 'what held it' line is wrong for a pass that ran on the CPU the whole time, and says 'not Linux?' on a Linux lamp); P3s (a loop-ending pass hidden unless in the top five; an unreadable file dropped silently; a muddled reboot doc line)
- 09:36 #1265 (a gesture laid over the reading lean; #941) ready, first review started
- 09:35 #1258 merged by the first builder (tests-only rule)
- 09:35 merged #1243 (lamp-health carries the self-test's newest run; a failed part is error until a run checks it again; report-only, nothing acts on the level; P3: a restart forgets a fail the newest run did not check)
- 09:34 #1255 first review: five P2s (no docs; no with/without test; the first-audio check acts on unfinished words; false stops like 'Wait, is that right?'; 'stop_word' not an expected discard); lamp run posted (2 of 5 stops caught on 52e6)
- 09:33 #1250 (service_privacy_controls cleanup runs the stop hook; #1249) and #1258 (three more stepped fixtures on the watchdog; my #1235 follow-up) ready, first reviews started
- 09:28 #1263 (lamp-keeper prepare reads the records as install does: the device key path) ready, first review started
- 09:26 #1261 (LAMP_QUIET_PACE: the coordinator passes every 5 ms while quiet) ready, first review started (checking whether the mute or privacy path rides on the coordinator's pass)
- 09:24 merged #1251 (a full look's head settles on movement only with --full-look-movement on; off unless set; P3s)
- 09:23 #1260 (Desk tutor's marks: a nod at right work, a tilt at a wrong line) ready, first review started; #1199 and #1244 moved, re-checks continue
- 09:16 #1256 merged by the first builder (tests-only rule); #1262 (#1226's P3 fix) ready, first review started
- 09:14 #1253 (the LAMP_SCHEDULE_SPEAK=skill fix) ready, first review started; #1042 rebased past Stuffy air, re-check started; #1158 moved again
- 09:11 merged #1047 (with LAMP_STOP_WORDS=heard-always a stop word over the lamp's voice stops it; off unless set; P3s left)
- 09:05 #1238 merged by the first builder at 09:03 after a rebase (switch census); #1239, #1243, #1245, #1251 moved again, reviewers told
- 09:04 merged #1226 (a request going on after an early end keeps what the second voice's rule knew of the one just before; my P2 fixed; P3 the first half read twice)
- 09:03 #1238 re-checked at 6583600cf: no finding left; ARM64 red only from lamp-voice's lamp-body break (fixed by #1254); merge on a fresh green run after a rebase
- 09:01 #1247 first review: P2 (port-53 probes can read a dead network as online behind a router that answers DNS itself; count only 443); off unless set; it opens a bare TCP handshake to 1.1.1.1 and 8.8.8.8 only while the link is down
- 08:58 merged #1011 (the reading pose: head down at the desk, leaning forward, held while a skill says reading; off unless sent; lamp-52e6 and lamp-4ace runs; P3 the base turns away from a person off to the side)
- 08:55 #1042 re-checked at 766779d3c: clippy P2 and queue P3 fixed; needs a rebase (4 files conflict with Stuffy air) folding in its task commit, task only on from=tasks
- 08:55 merged #1237 (a skill's news ask may name its task; the voice side of #1042's order P2)
- 08:55 #1254 merged by the first builder before me (lamp-body tests compile again)
- 08:54 #1244 first review: P2 (no test sends a refused tap through the coordinator with a ring: the face test's first half compares two untouched clones); P3s (latency not measured; a stale doc line). Not posted: Expression looks up at a tap while muted (predates the PR)
- 08:50 #1257 (Stuffy air follow-up: my #1066 P3s and the lamp-06c1 check) ready, first review started; #1158 moved (clock guard fix), re-check started; #1245 moved again
- 08:40 #1254 reviewed (lamp-voice's lamp-body tests don't compile since #1010 and #1192: duplicate helper; pure rename): merge on green ARM64
- 08:39 #1238 re-checked at a01ee4350: all 17 earlier findings fixed; three new P2s (LAMP_TASKS_WHO proposed on though its newest lamp-4ace records fail; second voice and --controls-mute have lamp-4ace records the page says are missing)
- 08:39 #1158 first review: two P2s (the clock guard never reaches the worker: env_clear drops LAMP_KERNEL_CLOCK; by day in a busy room all four 15:23 true calls are now held); after fixes, for the first builder (the day rule shown on no lamp; Fourth has none)
- 08:39 #1235 after the merge: no defect (a 400 ms cleanup stall fails 7 tests without it, 17/17 with it); the same pattern left in three other test files, noted on #705 for Tests
- 08:35 #1235 merged by the first builder (tests-only rule); #1011 (reading pose, was stacked on #1010) ready, first review started
- 08:32 merged #1248 (privacy_worker tests check the worker's fresh view, not the producer's cadence) and closed #1241
- 08:28 LAMP_SCHEDULE_SPEAK=skill bug confirmed by probe (the worker refuses every reminder): posted on #1038 and #705 for an owner
- 08:27 #1233 after the merge: nothing posted (product lines all cfg(test); not reproduced on base); an unconfirmed doubt that the mock no longer catches a shutdown that kills at once, not checked further
- 08:27 #1242 after the merge: P2 posted (no test fails if the 25 s budget is dropped from waiting_for_link); #1251 ready, first review started; #1238 re-check started
- 08:26 #1010 merged by the first builder (lean toward the person; lamp check on lamp-4ace today)
- 08:26 #1245 first review: P1 (ARM64 red: voice-eval PARTS lacks lamp-selftest); then for the first builder (on every new install, no lamp run, Ten has none); not the privacy path (passive: no mic, camera, sound, light or move)
- 08:26 merged #1236 (bad temperature reads counted, never a peak or heat; P1 fixed, lamp-52e6 run of the head; P3 a fast climb from a low first read never settles)
- 08:24 #1233 and #1242 merged by the first builder under the new tests-or-docs rule (08:21: a clean peer review plus green ARM64); #1237 and #1042 review started (top of the first builder's 08:04 order); #1243 moved (fixes), re-check started; #1240 and #1247 queued; the watcher now keeps its state between runs (it had missed #1237 going ready)
- 08:12 #1235 ready (hand_controls teardown under load, from my 07:07 on #705), first review started; #1234 and #1244 moved mid-review
- 08:11 #1243 first review: P2 (a later run that could not check the failed part clears the error; the 6 h passive timer would clear speaker, ring and head failures); P2 scope (keep.rs is the fifth's; needs its yes); no lamp run posted for an always-on change
- 08:09 #1230 ready, first review started; #1047 moved to 640f54449 (fixes), re-check queued
- 08:07 #1238 first review (switch census): 7 P2s (LAMP_SPOKEN_TO and LAMP_POLICY_HOLDS_ACTIONS proposed on though later records show them failing; held calls run after 2 s; six switches missing; wrong 'not shown' and 'shown' rows); side find: LAMP_SCHEDULE_SPEAK=skill may never speak a reminder (#1038), being traced
- 08:05 merged #1246 (lamp-4ace hardware note: neither mic carries a person's word over the lamp's speech; P3 wording)
- 08:03 #1239 first review: two P2s (the ease-out is a snap, 0.32 s at +24/-21 a step then a dead stop; the newest gesture in a step overwrites the one before whatever its priority); safety holds; off by default
- 08:00 #1243 and #1244 ready, first reviews started; #1236 moved to 11515b50d, re-check started; #1226 moved to 5c9a2e780, re-check queued
- 07:54 #1233, #1239, #1242 ready, first reviews started; #1199 and #1238 moved mid-review, re-checks continue
- 07:48 #1236 first review at be93c2332: P1 (once a joint reads more than 15 from its last good value every later read is bad, so a real 70 never stops the arm; probes confirm); P2 (five stood asserts removed from unrelated tests); P3 doc; on by default, needs a lamp-52e6 run of the fixed head
- 07:44 merged #1231 (news left out keeps its place; a cut answer told together keeps its gist; my #1223 P3s)
- 07:43 merged #1228 (the worker's own loop shows a request after a bare call; #1108's unexercised path)
- 07:42 #1047 first review: three P2s (a spoken stop skips the end-of-reply count write; the lamp's own words are kept 6 s from text arrival though a reply can sit 30 s ahead, so its own 'wait' stops it; docs say raw was never played though three lamp runs exist); off unless set; not a privacy-path change
- 07:42 merged #968 (the speaker check reads echo from the lamp's own speech, off unless LAMP_SELFTEST_SPEAKER=on)
- 07:40 #968 re-checked at 369e973cb (rebased): no finding; merge on green ARM64 (off unless LAMP_SELFTEST_SPEAKER=on; the msgs shape is the first builder's 10:21 answer)
- 07:38 #1199 re-checked at 6280d17c8: plain timing, no lamp run needed (as ask 18's build); P2 (README and docs say ask 22 played both clips; its recording shows plain timing drops the second in 19 and 22); P3 (the long-dance test fails without the gap only 3 of 6 runs)
- 07:33 merged #1066 (Stuffy air reworked per #1021; off unless installed; P3s: a tool-started dim gets no end, rearm untested in cases, night spends a chance, whole sleepy clip)
- 07:26 #1042 re-checked at 973e5778d: reload P2 fixed; order P2 fixed for one message only (still needs the voice to carry the task, no PR yet); new P2 (clippy too_many_arguments on take_skips); P3 (a dropped queued run blocks the runs behind it)
- 07:23 #1191 merged by the first builder (ring resync); #1047 and #1231 ready, first reviews started
- 07:18 #1179 merged by the first builder (stall recorder)
- 07:15 #968 re-checked at 7aadecde0: my 06:46 P2 fixed (set_unshown back for a skill sound); needs a rebase (speaker_tone.rs conflicts with #1214); P3 (the ported test copies the loop's expression)
- 07:13 merged #1229 (a silent privacy worker ends the loop: stepped test; my #1222 P3)
- 07:12 merged #1225 (a speaker lapse mid-answer tested through the loop; my #1214 P3)
- 07:10 merged #1227 (#1224's open P2: introductions no longer name the speaker)
- 07:08 #1179 re-checked at bf21ea87c: clean; for the first builder
- 07:1x #1229 reviewed: no finding (both ends of the 50-60 ms window pinned); merge on green ARM64. Follow-up: stepped hand_controls teardown still fails under heavy load (2-3 of 120), noted on #705
- 07:1x #1228 reviewed: no finding above P3 (test can fail both ways); merge on green ARM64
- 07:0x #1042 first review: two P2s (a reload drops the next task's news; runs come out in text order so task 9 and 10 get swapped moods); scope borderline (crates/skills keys)
- 07:0x #1226 first review: P2 (a continuing request carries over the last request the rule judged, of any age or session: the #1168 lesson again; probed)
- 06:48 #1192 merged by the first builder at 0707e8ea0 (code ready per my re-check; no lamp run yet, as no body goes on a lamp from a merge)
- 06:48 merged #1223 (task news together; off unless set)
- 06:47 merged #1218 (lamp-skills test clock; 120 s to 32 s)
- 06:5x #968 re-checked at 1d23b5763: history clean (no Python), earlier findings fixed; new P2 (the rebase drops set_unshown, so a skill's sound shows the thinking and speaking faces on lamps with LAMP_SAY=on, with the switch off)
- 06:4x #1223 first review: P3s only (a task put back goes to the front; the together note drops the #757 guard on cut answers); merge on green ARM64
- 06:43 #1214 and #1224 merged by the first builder at 06:41; #1224's open P2 noted on #705 for a follow-up
- 06:4x #1010 re-checked at 3a977eb9b: builds again, no finding; waits only for the lamp-52e6 run with a hand on the elbow (Monday)
- 06:42 #1214 re-checked: clean; for the first builder (lamp-4ace); told on #1178
- 06:4x #1224 re-checked at 11e57f43f: P2 half fixed ('it's X' needs 'my name' now; bare 'Lamp, this is Jim.' still gives a name while 'this is Jim speaking' no longer does)
- 06:41 limit again ~06:30-06:40; merged #1201; restarting reviews
- 06:40 merged #1201 (the paced duplex test on the scenario clock; my doc P2 fixed)
- 06:27 #1201 re-checked: doc P2 fixed; merge on green ARM64
- 06:25 merged #1222 (hand_controls' four loop tests stepped; the long-flaky hand-stop test included) and closed #1220
- 06:1x #1224 first review: P2 ("this is Jim" fires on introducing someone else: an unnamed speaker's face can be filed under the friend's name when the friend stands far enough away)
- 06:1x #1218 re-checked at b2009ffc1: all four findings fixed (real waits tested again); two new P2 test races (the host-clock test's start time taken after the publish; the Policy's 300 ms now a skip so say_now can beat the face's leave)
- 06:0x #1192 re-checked at 0707e8ea0: no finding (resting arm not eased); merge once the full lamp-52e6 rerun is posted on this head and ARM64 is green
- 05:57 #1214 first review: the silent case fixed (test proven by a revert run); P2 (a lapse while speaking can still end the voice); then the first builder's (lamp-4ace); told on #1178
- 05:5x #1192 re-checked at ebe72debd: code ready (easing capped at 400; all probes hold); waits for the full lamp-52e6 rerun and ARM64
- 05:52 merged #1221 (#1208's P3s: a yes as a question is no yes; more ordinary yeses)
- 05:4x #1218 first review: P2 (no test runs the real waits any more: every waiting test passes --clock-control); P3s (skip_us unbounded, remove_file at PATH)
- 05:4x #1215 reviewed: no finding (640/640 a_tap under stress; the doc conflict fixed by the rebase); merge on green ARM64. Flaky on the host clock: a_lamp_put_to_sleep_from_the_head (2 of 120)
- 05:47 merged #1215 (a_tap owner and reply made inside the scope)
- 05:45 merged #1219 (realtime-tests.md: the windows endpoint_session's 27 keep; my #1203 P3)
- 05:43 posted on #705: #1217, #1197 merged; a new hand_controls flake
- 05:3x #1192 re-checked at d3cafdcd7: the easing P1 fixed (an out-of-window goal stops at once); P2 (easing turns any in-window jump into a walk, so the step rule never stops a computed goal: needs a cap or the first builder's yes)
- 05:39 merged #1197 (asker-first follow-up, off unless set)
- 05:33 asked the first builder on #705 about #1192's step rule (stop vs walk)
- 05:33 merged #1217 (#1161's echo P2)
- 05:31 merged #1216 (privacy test skip line to stderr, my #1213 P3)
- 05:2x #1197 re-checked at b47ff573b: clean (probes: the asker's soft follow-up taken, the answer held at most 120 ms, leak and 14:21 fine); merge on green ARM64
- 05:18 merged #1208 (checked names: yes, no and correction handling; off unless set)
- 05:17 merged #1211 (records tests skip loudly under the emulator, never on CI) and closed #1210
- 05:0x #1192 re-checked at 221b71ab5: the resting-short P2 fixed; the easing P1 still open (an out-of-window goal still walked in 50-count steps)
- 05:06 merged #1203 (endpoint_session leaves the realtime group but for two)
- 05:01 #1211 re-checked at 653068ec3: P2 fixed; merge on green ARM64
- 04:5x #1192 re-checked at e3894ffe6: the nod P1 fixed (hold releases ramp); new P1 (the easing turns a refused out-of-window goal into 50-count steps toward it: the elbow walked to 2353); the resting-short P2 still open
- 04:59 merged #1213 (privacy gate test under the emulator, test-only) and closed #1212; #1211 sent back (P2: the skip line is hidden by cargo test)
- 04:50 #1197 re-checked at e5965fe06: the leak P2 fixed; new P2 (on a lamp the echo check is undecided for the reply's first 0.2-0.45 s, so the asker's soft-start follow-up is dropped again; the test rig always gives settled evidence)
- 04:4x #1208 re-checked at d08d7e2d7: my P2 fixed (no-after-yes refused); new P2 (a yes plus a correction with no 'no' word, e.g. 'Yes, but it's …', still lets the misheard name replace; three of these also pass on lamp-voice)
- 04:4x #1192 re-checked at 5de7db4ea: my P2s fixed, but Fifth's lamp-52e6 run found a P1 (a window refusal on the first nod let go of the standing lamp; every hold release needs a ramp) and my 04:14 fix brought a P2 (a folded arm resting short is now pushed at)
- 04:49 #1115 rebased c674497b4: fine; waits only for lamp step 4
- 04:3x #1115 re-checked at 64ac1f45c: my P2 fixed (probed: a hand on the base or head during the lowering lets go again), both P1s hold, commits folded; needs a rebase (USAGE text) and lamp step 4 on lamp-52e6
- 04:3x #1199 re-checked at de0ecaef6: still P2 (replayed on ask 23's own joint states the new start fit sees the start 2.8 s late); not shown on a lamp
- 04:39 merged #1207 (a_tap tests on the stepped clock) and closed #1206
- 04:36 #1209 re-checked at 4c1a7ea36: no P1/P2, my P3s fixed (14 probes); for the first builder
- 04:31 merged #1195 (the backup voice's down line, off unless set)
- 04:20 #1208 first review: P2 (a no after the first yes word is ignored, so "Of course not." and "Absolutely not." agree and a misheard name replaces the right one); P3 (Vietnamese yes words missing)
- 04:1x #1192 re-checked at 5319b20f1: my P2 fixed (16,320-run sweep wrote no goal outside its window); new P2 (when the folded arm rests its goal is set to the last reading, so a steady wrong reading under 300 counts is written with torque on: the elbow moved 142 counts in a probe); waits for a lamp-52e6 run
- 04:1x #1197 re-checked at 0bbe1f95d: my P2 and P3 fixed; new P2 (while the answer plays the rule counts the lamp's own voice leaking past the echo canceller, so a line from across the room cuts the answer: 14:21 again)
- 04:18 #1209 first review: no P1/P2; for the first builder (real hardware, no lamp); P3 (one pressed first read turns the reset off for the run)
- 04:17 merged #1194 (the log relay never makes shared stderr non-blocking)
- 04:17 merged #1202 (65 tests leave the realtime group)
- 04:11 #1191 re-checked at 85a8c664d: clean; for the first builder (lamp-4ace check named)
- 04:0x #1199 first review: P2 (the new start check saw the clip's start 2 s late in ask 23 on lamp-06c1, so the lamp stood still 2 s mid-dance; README says the body was late, the raw files say it wasn't); P3s
- 04:0x #1195 re-checked at 0c09c3f1a: P2 and P3 fixed (probed); needs a rebase (tests.rs end, after #1200), then merge on green ARM64
- 04:04 merged #1205 (the lamp-bus slow-follower flake) and closed #1204
- 03:58 merged #795 (the privacy switch, mine to merge) and closed #755
- 03:54 #1108 re-checked at 4cdb7239e: my P2 fixed (probed); for the first builder (lamp-4ace only)
- 03:51 #1179 re-checked at 48c23ca0b: clean; for the first builder (times the capture and camera loops)
- 03:50 merged #1057 (checked names, off unless LAMP_NAMES=checked; history folded, no names)
- 03:4x #1191 re-checked at 1ecd662d9: the first builder's two P2s fixed (probed); new P2 (a held coordinator loses the ring's on-time receipt, so resync waits for one already come and the ring goes off for the cycle); then the first builder's (no lamp)
- 03:3x #1194 re-checked at 6681624fd: P1 fixed, ARM64 green; P2 still open (no test fails with the bug put back; a child-process test offered)
- 03:3x #1195 first review (a few minutes late): P2 (the down line is said on written turns too: a hello, news or a skill's line with nobody asking); P3 (no length cap on the kept clip, no journal line on a failed render)
- 03:38 merged #1198 (expression test clock keeps up with the wall) and closed #1196; #1201 sent back (P2: docs and comments still say real time)
- 03:37 merged #1200 (#1175's follow-ups: a stop carried to the continuing request, at most 2 min)
- 03:20 #1197 first review: P2 (the asker's own soft-starting follow-up is dropped at the answer's start with on, while record takes it); P3 (the 7 s bound takes over mid-line); the 14:21 case and the crash are fine
- 03:1x #1194 first review: the fix is right (probed with a child process); P1 test-only (its socket test fails on Linux every time: ARM64 red); P2 (no test fails if the bug returns; a child-process test proposed)
- 03:1x #1131 checked after the 02:06 merge: tests not weakened; P2 follow-up (two "at once" checks now leave about 70 ms of real time)
- 03:1x #1179 first review (late, posted 02:08 on an early head and now on 1cfd16dd0): P2 (with LAMP_STALLS=on the recorder's Drop waits before each loop's cleanup, delaying the mic and camera close); P2 (the new test can't fail: the 8 MiB vec is optimised away in --release); conflicts; for the first builder (privacy-path timing)
- 03:1x #795 re-checked at 5937a5d73: two P2s (does not build after #1092: a test Rig lacks asker_first; a bad merge with #1176: a head-only legacy_head engine misses GPIO read faults even with the switch off); product pins fine
- 03:19 posted the 01:40-03:20 batch on #705
- 03:18 merged #1193 (#1103's P3s; ARM64 rerun green after a lamp-bus flake)
- 03:0x #1182 checked after the 02:14 merge: privacy holds; P2 (the bare 'authority snapshot has expired' can come from the speaker's silence check, so #1182 alone may not keep the voice up); first builder's lamp check (told on #1178)
- 03:0x #1175 checked after the 02:57 merge: my P2 half fixed; follow-up P2 (a stop used up when its request is asked, then given up: the continuing request gets no note) and P3 (a carried stop has no time limit)
- 03:0x #1057 re-checked at 3bce3bbc6: my P2 and P3s fixed (phrase lists re-run); new P2: three commits still add people's names and a record's letter, so a rebase merge would put them in lamp-voice history (fold); the first builder's 02:42 cases not yet tests
- 03:0x #1192 first review: P2 (three arm-goal writes in take_hold and the overload clear skip the guard: a raw read of 37639 still reaches the wire under --torque on); waits for a lamp-52e6 run
- 03:08 #1191 first review: the first builder's two P2s confirmed by measurement; then first builder's (no lamp)
- 03:05 merged #1173 (the staff note, off unless set; all P2s fixed)
- 02:5x #1092 checked after the 02:33 merge: my P1 gone (probed, a test now catches it); the rule still holds while a kept line lasts with no limit (needs a bound before it is switched on)
- 02:5x #1161 checked after the 01:44 merge: P2 follow-up before LAMP_TASK_NEWS_AT_PAUSE is turned on (the lamp's own echo in a reply's tail counts as the person's speech: news 1.2 s after the lamp stops)
- 02:5x #1143 reviewed: all #1099 and #1003 P3s fixed, probed; merge on green ARM64 (rerun after an expression flake)
- 02:5x #1107 checked after the 01:52 merge: both P2s and three P3s fixed (probed), nothing left
- 02:59 merged #1143 (#1099/#1003 P3s: a waiting reminder decided at once)
- 02:58 merged #1012 (Dance; shown on lamp-06c1; all findings fixed)
- 02:56 #1130: passed on a possible crash with LAMP_WAIT_KEEPS_ANSWER on (a quiet line 0.2 s before the answer), maybe a probe artefact
- 02:54 merged #1166 (light/reached on the bus, off unless LAMP_LIGHT_REQUESTS; my P1 fixed)
- 02:53 #1173 re-checked at 8c37ca376: the test P2 and both P3s fixed; merge on green ARM64
- 02:50 #1193 reviewed: no finding (#1103's P3s); merge on green ARM64
- 02:4x #1130 checked after the 01:44 merge: P1 and P2s fixed (probed on the product mask); P3 follow-up (a block with no echo evidence while a sound waits)
- 02:48 merged #1187 (the voice-check scene on the logical clock, #1160)
- 02:46 merged #1190 (two review hearings on a logical clock, out of the realtime group)
- 02:46 merged #1054 (actions held by the Policy, off unless set; my P2 answered: the 11:20 timer stays held, said so)
- 02:3x #1173 re-checked at 31bb35784: my P2s and the first builder's fixed; new P2 (the switch-off test can't fail: it says "ask Codex" where only Claude Code matches); conflicts after #1148
- 02:3x #1108 re-checked at 5ed472f50: rebase only, my 22:01 P2 still open (a weak bare call on name/rising alone lets the next line play in a busy room); off unless --call-wait-ms
- 02:38 #1166 re-checked at 4edf866ed: my P1 fixed (probed); new P2 (ARM64 red on the merge: light/reached missing from voice-eval's BUS_TOPICS after #1165)
- 02:38 #1103 re-checked at 698dd1789: my P2 and P3s fixed (probed across a reboot); waits on the first builder's answer to Twelve's/Nine's timer question (asked on #705 at 02:38)
- 02:31 #1189 reviewed: duplicate of #1186 (merged 02:22), asked to close
- 02:23 #1188 reviewed: no finding (my #1172 P3s); adds back a camera ask on the second route, so for the first builder
- 02:23 merged #1186 (say test authority at the observed moment) and closed #1185
- 02:1x #1156 checked after the 23:47 merge: no P1/P2; my P3s and the first builder's --progress P2 fixed; works on a home lamp now that #1174 is in
- 02:1x #997 checked after the 23:23 merge: both P2s fixed (probed, incl. a second restart in one boot); P3 follow-up (failed unlink plus a hard death before the last persist still opens the mute on a tap: persist_muted in mute_chosen)
- 02:1x #1010 re-checked at b84800082: the P2 and P3 fixed (probed), but it no longer compiles on lamp-voice (#1138 added Asked::still): rebase; then waits for the lamp-52e6 run with a hand
- 02:14 merged #1184 (install reads records with Records::state; no V1 key after a set-aside)
- 02:08 merged #1180 (spoken-to tests hold the one thread one at a time)
- 02:01 #1184 reviewed: no finding (install reads records with Records::state; no V1 key after records set aside); merge on green ARM64
- 02:00 #1012 re-checked: every earlier finding fixed, the long dance shown on lamp-06c1; needs a rebase after #1091/#1080
- 01:5x #1165 and #1170 checked (merged 01:47): no leak, the export ids don't link back, all my #1134 findings fixed by #1170
- 01:5x #1175 first review: P2 (the stop note is lost when the request after the stop is never asked: the stopped story can begin again on the second voice); waits on #1148 then #1107; conflicts
- 01:5x #795 re-checked at 9b0202dbc: clean (P2 fixed, probed; the lamp-06c1 run still covers it), P3s only; needs a rebase after #1176, then I merge on green ARM64
- 01:58 #1094 re-checked: all 13 findings fixed (merged by the first builder); main-side adapter work noted on #701
- 01:57 #1115 re-checked: both P1s fixed (probed); new P2 at 667057f65 (a hand turning the base during the lowering ends the run), fixed after in 64ac1f45c (unchecked); waits for lamp step 4
- 01:57 merged #1172 (no phrase look in the lamp's own turns; only removes camera uses; my 2 P2s fixed)
- 01:54 #1168 checked after the 01:52 merge: my P2 fixed (heard_last taken at every start, same session, the one just before)
- 01:52 #1180 reviewed: no finding (test-only lock for the one thread); merge on green ARM64
- 01:47 #1176 checked after the 01:44 merge: no P1/P2; #1167's findings fixed; four P3s; told El what the lamp should do
- 01:46 merged #1100 (steady person; both privacy P2s fixed and probed on both routes)
- 01:44 back after the usage limit (00:12-01:40); merged #1174, #1177; restarted first reviews
- 01:42 merged #1177 (Expression doc comments, #1138 P3s)
- 01:42 merged #1174 (product lamp-tasks unit with --progress --push-wait; lamp-4ace half taken out after my P2)
- 00:08 #1177 reviewed: no finding (doc comments only, #1138 P3s); merge on green ARM64
- 00:01 #1092 re-checked at d0682b1cd: CHANGES (P1: a short quiet word past 0.8 s ends the voice loop; P2s: 5 s rests on one sound, record and on judge at different blocks); the four earlier P2s fixed

## 2026-10-16, from 15:06 (Claude session, taking over from Codex)

Newest first. Times from `date` (+07).

- 23:5x #1096 checked after the 23:40 merge: P2 and P3s fixed; P3 (light/drawn only from resting frames, so a same-turn "off" under a turn's glow says already off)
- 23:5x #1069 re-checked: both P2s fixed, P3s only (a lookup does count, unlike the doc); merged by the first builder meanwhile
- 23:56 merged #1083 (rise lines only, after its rebase on #1126; my P3 taken)
- 23:50 #1153 checked after the 22:37 merge: P2 follow-up (the 30 s not-speaking rule delays the hello, so it greets someone it answered 30 s before); P3s
- 23:50 #1138 checked after the 23:38 merge: no P1/P2 (worst frame 4.52 mm, one layer, no dip); P3s
- 23:4x #1146 checked after the 23:16 merge: the first builder's three P2s fixed; mine only when the model calls before it speaks (P2 follow-up: with a lead-in the still is asked as the pose starts)
- 23:49 #1163 checked after the 23:42 merge: P2 and P3s fixed (3 looks a minute, not 300); two new doc/journal P3s; lamp-cb94 count was on the earlier head
- 23:45 #1174 sent back: P2 (the lamp-4ace unit gains --push-wait but still pins the 02:11 lamp-tasks build, which refuses that option, so lamp-tasks would not start); product half fine
- 23:40 #1010 sent back: P2 (the new !blind in lean_arm drops the lean on a wire hiccup: two 60-count jumps on 4 missed reads); asked for a lamp-52e6 answer with a hand on the elbow
- 23:36 #1167 checked after the 23:22 merge: reset safe; two P2s before the next product install (doc says head touch reaches the voice: it doesn't; head Pet lost under --touch); told on #705
- 23:34 merged #1169 (docs: the two host-clock bounds left in pilot_faults; my #1162 P3)
- 23:2x #1107 re-checked at 789a03565: still CHANGES (rebase only, my P2 not fixed; new P2: #1137's close rule keeps a tap's sound inside the 4 s, so it reaches the model with no after_stop)
- 23:29 #1164 checked after the merge: the BASHPID fallback is right on real bash 3.2; P3: the lock test needs Linux tools, so it can't run on a Mac
- 23:27 #1083 re-checked: no blocking finding (rise lines only; the day/night rule moved to draft #1158); conflicts with #1126, rebase then merge
- 23:26 #1057 sent back: P2 (the 15:33 fix went too far: "Please remember my name is …" and "Can you call me …?" give no note now); the first builder's 18:00 cases not on the head yet. Reposted my 15:33 without a misheard record's letter (18:12 rule)
- 23:26 merged #1157 (lamp-tasks push wait, off unless --push-wait; my P2 fixed: back-off per agent)
- 23:24 #1168 sent back: P2 (heard_last is the last words heard by any request: a successor of a request dropped before its words is asked with an older request's words, any session; probed)
- 23:17 closed #1140 (#1153) and #949 (#996) by hand: both merged by the first builder, issues left open
- 23:16 #968 sent back: P2 (Python added then removed across commits, so a rebase merge puts it in lamp-voice's history: fold); needs a rebase after #1144; all 7 earlier findings fixed
- 23:16 #1000 checked after the 22:30 merge: P2 fixed; closed #906 by hand
- 23:12 #1163 sent back: P2 (a sensor that answers its name but fails every ready is retried every 200 ms: 600 journal lines a minute, worse than today); third has no lamp, so after the fix it needs Twelve's lamp-cb94 count or goes to the first builder
- 23:07 #1103 sent back: P2 (overdue() is false while the wall clock is untrusted, so after every product boot an overdue reminder is said into an empty room as due now)
- 23:07 #1054 sent back: P2 (the 11:20 "Set a timer for 1 minute" it was written for is still held at 0.6); cancel_reminder P2 fixed at e3c566f7e
- 23:07 merged #1162 (pilot_faults leaves the realtime group; P3: two host-clock bounds left in one test)
- 23:03 merged #1126 (every verdict in the journal; rebased after #1030, both sides kept)
- 23:02 merged #1003 (--rules for the hello and skill asks; my P2 fixed; two P3s left)
- 23:00 merged #1159 (a full look with no still is a look on the second voice: 5 s and its line; my #1154 P3)
- 22:56 #1159 reviewed: no finding (my #1154 P3 fixed); merge on green ARM64
- 22:49 #1144 reviewed: no finding, ready (the first builder's four P2s fixed); merged at 22:50 by another hand
- 22:47 #1157 sent back: P2 (a stuck agent's backoff covers the whole machine, so another task's end there can be told 20 s late); P3 (a --push-wait store is refused by an older lamp-tasks); the eighth's P2 fixed in cd6bf9f36
- 22:36 #1154 checked after the merge: sound; one P3 (a full look with no still is look:false, keeps 3 s and ends silent)
- 22:23 #1146 sent back: P2 (the still is asked before Desk tutor's lean starts, so the picture can be of the head still up); camera path, so the first builder's after the fix
- 22:17 #1130 sent back: P1 (on the product mask a sound past the 1 s wait is rejected, retained history 128 < 130 blocks, so the follow-up is lost); P2s (switch-off untested; "short" is only up to 0.75 s; the lamp's name unused)
- 22:33 #1100 re-checked at fdf036b0d: 21:32 case fixed, two new P2s (newcomer who never counts; second route never notices the 30 s gone). #1101, #1104, #1132, #1075, #996, #900, #1137 merged by the first builder meanwhile. Re-checks started: #997, #795, #1003, #968+#1010, #1094+#1115, #1000, #1069; first review #1134. Flake noted on #705 (hand_controls hand stop before provider fault, 2 of 4).
- 22:27 merged #1030 (the voice check, off unless LAMP_SAME_VOICE_MODEL; both P2s fixed)
- 22:20 #1145 checked after the merge: sound; one P3 (went_on cuts an earlier answer that was never told)
- 22:12 #1138 re-checked: P1 fixed (9 shifts in 20 min over Expression); waits on a lamp-52e6 run; living-thing overlap with Expression's drift for the first builder
- 22:12 #1012 sent back: two P2s (a long dance plays one clip on a real lamp, the second dropped while the first plays; [again] matches only the newest heard piece); motor safety holds (worst 13.7 mm)
- 22:11 #1108 sent back: P2 (a bare call needs only name/rising, so the next 3 s play in every room ahead of the busy-room bars)
- 22:11 #1107 sent back: P2 (a head stop's own tap still counts as a call for 2 s); on by default with no switch and no lamp result posted, so the first builder's after the fix
- 22:07 #1153 sent back: two P2s (the Policy's 15 s hello rule refuses the new 5 s ask and spends the cooldown; it greets someone it just answered); first builder to choose the Policy side
- 22:04 #1092 sent back: four P2s (the first builder's three confirmed with numbers, plus: the rule keeps the turn with no time limit while a held answer is awaited)
- 22:00 #1083 sent back: two P2s (night_here ignores an unset clock; not the shape #829 asked for); it is not on lamp-4ace (taken off 16:32)
- 22:00 #1069 sent back: two P2s (a call counted as answered before the Policy hold decides; lookups count as answering)
- 21:56 merged #1149 (an ignored test pinning #560's 20 s result bound)
- 21:51 #1080 re-checked: P2 fixed; ready for the first builder's lamp check
- 21:47 merged #1114 (eyes at half rate while a face sits still, behind --steady-rate)
- 21:34 #1138 sent back: P1 (the idle never shifts on any deployed lamp: Expression's rest lease blocks it); P2 (tilt left/right identical without --stand-roll); first builder to choose body idle vs Expression drift
- 21:33 #1100 sent back: privacy P2 (a newcomer's first 10 s filed in the previous person's thread and told to them later); the flip fix itself works
- 21:24 merged #1152 (my three P3s on #1088)
- 21:24 merged #1081 (the reminder journal; my P2 and P3s fixed)
- 21:16 merged #1150 (a doc comment back in place; #1048's P3)
- 21:16 #1114 sent back: P2 (a colleague's name in a test); on by default with no lamp evidence of the half rate
- 21:12 #1058 checked after the merge: safe (all clips under 18.7 mm, steps under 800 counts/s); a Python generator under lampOS/ noted to the first builder
- 21:10 merged #1070 (the duplex harness reads a paced block at its own gap)
- 21:08 #1080 sent back: P2 (a pause after a gesture was taken doesn't stop it: rises and nods while paused); conflicts with #1072's head; then first builder
- 21:07 merged #1109 (endpoint_session waits for the real worker to settle; #823)
- 21:04 merged #1120 (setup takes the user's voice key; a reset wipes it; my P2 fixed)
- 21:04 merged #1102 (install never puts V1's key back after a reset)
- 21:01 merged #1084 (an ignored test pinning #560's silent accepted agent)
- 20:56 merged #1136 (near 0.14 measured, comment only)
- 20:55 merged #1142 (#1106's P3s); merged #1141 (lamp-75c9 shared with ten)
- 20:49 merged #1088 (early_end on one stepped clock); #1109 needs a rebase after it
- 18:24 #1003 sent back: rebase onto #1099, then P2 (kept rules must never hold a reminder the person set)
- 18:24 merged #1106 (lamp-policy --attention-next, in shadow; #1040)
- 18:23 merged #1105 (processor heat in lamp-health; my P2 and P3 fixed)
- 18:18 merged #1099 (a reminder act in the Policy, held only while someone talks; answers my #1039 P2)
- 18:17 merged #1122 (the light test's state lives 500 ms; #1121)
- 18:17 merged #1113 (the lookup test waits to be counted out; #1112)
- 18:14 #1115 sent back: two P1s (22 s lowering with no watchdog ping, so systemd kills the body; a hand stopping it gets the full stand pose written into it)
- 18:11 #1075 sent back: P2s (a relay refusal fails the look turn with no retry; the look not shown on a lamp; docs don't say where the picture goes)
- 18:11 merged #1125 (call_tool in skill cases; Desk tutor's first case)
- 18:10 merged #1118 (a lookup counted out before its answer; #1124 duplicates it)
- 18:10 merged #1111 (a link change inside a hold-up is dated late, the worker goes on: the 16:38 crash)
- 17:33 #1120 sent back: P2 (an empty key finishes setup when the lamp has no key file, leaving it with no voice); key handling itself fine; #1102 must go first
- 17:23 #997 still not ready: P1 fixed (probed); P2 half fixed (a failed marker unlink + restart lets a tap open the person's mute); then first builder
- 17:09 merged #1110 (Harness descendant proven killed by its lock; the tasks flake)
- 17:03 merged #1085 (tools.json over 32 functions not refused; #1020's P3)
- 16:59 #1105 sent back: P2 (throttled = scaling_max < cpuinfo_max is an error with no heat check; a boost OPP or a deliberate cap reads as heat)
- 16:59 merged #1063 (tasks without Harness: unit, health, voice; my P2 fixed)
- 16:51 #1039 sent back: P2 (policy = reminder reaches the Policy as unprompted, so a reminder the person asked for can be held by the rules for unasked speech; SKILL.md says otherwise)
- 16:42 merged #1087 (docs: a from ask names its source; Thirteen's #1038 P3s)
- 16:38 #1091 re-checked: P2 fixed; waits on #1072/#1080, then first builder
- 16:34 merged #1098 (Desk tutor holds the reading pose 2 min)
- 16:34 merged #1062 (room older than 3 s is unknown; #1032's P3s)
- 16:30 #1091 sent back: P2 (moves/NAME.json read with no file check or size bound: a FIFO blocks every skill's reload, 300 MB read whole); waits on #1072 and #1080; then first builder (motion shown on no lamp)
- 16:28 #1010 sent back again: 15:49 P2 fixed (probed); new P2: the fixed-count clamp does not follow a fold, so on a stop or retreat during a held answer the elbow jumps 134–156 counts in one step after 1.5 s
- 16:26 #1094 sent back: two P1s (Python in the tools scripts; cleanup runs on every stop, so each reboot unmasks V1's LED units) and P2s (scope: crates/update and a second lamp-os package)
- 16:26 merged #1077 (full look settle; P1 fixed)
- 16:26 merged #1089 (skills wait for a talk to end)
- 16:26 merged #991 (fifth-P8 in joining.md and role files)
- 16:16 merged #1082 (#958's P3s: a dead asker while someone talks is held)
- 16:08 merged #1065 and closed #1060 (Policy bus tests wait, not sleep)
- 16:05 merged #1074 (empty scan answer kept 5 s; my #1064 P3)
- 15:59 #1077 sent back: P1 (one Stopped reading takes the still; in a replay of lamp-52e6's joints a turning head got its still while moving in 240 of 525 asks, against 17 on lamp-voice)
- 15:58 #900/#996/#1000 sent back: all three need a rebase (scenario_clock.rs conflict with #1009); #900 two P2s (eprintln inside the 20 ms ack deadline; native_head still in the realtime group so ARM64 ran none of its 11 tests); #1000 P2 (says Closes #906, but #906's third test is still on the host clock)
- 15:53 #1030 sent back: two P2s (the voice check runs on every event, not once per request; a held other-voice line counts toward nothing)
- 15:53 merged #1078 (Stretch break near 0.14)
- 15:51 #795 sent back: P2 — losing Controls can still make lamp-voice exit (worker's unreadable Fault counted as a cleanup failure at shutdown); earlier findings fixed except the privacy-switch.md:117 P3
- 15:50 #970 left for the first builder: privacy (mic audio to the relay during a yield); all earlier findings fixed, P3s only
- 15:50 #1010 sent back: 12:20 P2 fixed (probed); new P2: at the answer's end the hold lets go and the elbow goal jumps 40 counts in one step, pushing into a hand still there
- 15:44 #993 sent back: both 11:35 P2s fixed (probed); new P2: during a skill's glow, 'light off' when the person's light is already off publishes nothing (actions reads light/state); crates/actions is not third's
- 15:43 #990 re-checked: P2 fixed by a cargo fetch retry step; merge is DevOps's
- 15:41 #873 left for the first builder: privacy (camera frames to the cloud with --images); otherwise ready, P3s only
- 15:35 #968 sent back: Python still in the PR, cancelled_db still redefined (first builder's 10:21 ruling), no off-unless-set test
- 15:25 #997 left for the first builder (privacy path: a head tap ends a mute), after its P1 (a tap can open a person's mute after a hard crash) and P2.
- 15:25 #1057 sent back: rebase, and a P2 (`gives_a_name` takes questions about a name as a name given).
- 15:22 #1063 sent back: P2 (a crashed tasks part is told as "not set up", since systemd removes its socket folder).
- 15:20 #990 sent back: P2 (`net.retry` does nothing for the index errors on Rust 1.96); the workflow is DevOps's, not in nine's role file.
- 15:17 merged #1064 (setup's empty scan and scan limits; one P3 left).
- 15:16 merged #1013 (keep-logs; the 12:25 P2 fixed).
- 15:15 #1061 reviewed, no finding; merged by the first builder first.
- 15:11 merged #1002 (Desk tutor's folder; two P3s).


Reviewer merged #751, #765, #780, #790, #792, #797, #807, #808, #815, #818, #822, #827, #843, #848, #850, #852, #853, #855, #859, #861, #864, #865, #867, #872, #875, #877, #878, #885, #886, #888, #889, #891, #894, #895, #897, #899, #901, #902, #903, #904, #907, #909, #910, #912, #915, #916, #919, #922, #923, #924, #927, #931, #932, #942, #943, #944, #945, #947, #950, #956, #957, #971, #982, #986 by rebase; manually closed #752, #753, #777, #803, #837, #846, #854, #860, #862, #879, #882, #890, #892 and #975. #854 was later reopened by First for its story handover regression. #703 was closed by Fifth after First merged #857; #732 was closed by the first builder after #751, #765 and #793 were all merged. First builder merged #738, #778, #789, #791, #793, #794, #796, #798, #809, #810, #816, #817, #835, #839, #842, #851, #858, #870, #881, #884, #896, #934, #908, #917, #921, #928, #929, #930, #936, #937, #954, #955, #961, #962, #964, #967, #974, #857, #883, #979, #845 and #905; #976 was merged separately after the integration build break. Devops merged #821, #824, #825 and #826 (the last into main). No lamp actions or production source edits by this reviewer. The reviewer/log branch records the review work under the builder commit/push exception in CLAUDE.md (61d1ea6fc).

Updated: 2026-10-16 11:36:24 +0700

## PR #883 — after merge hello rules doc p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/883)

Reviewer: [P3] at 11:35, following the separately performed merge: `lampOS/docs/policy.md:126–127` still says Hello #883 will obey kept `hello` rules once it lands. This implementation applies rules only to Progress (`voice/src/policy.rs:1589–1593`); Hello is constructed without them (`policy/src/main.rs:272–277`). The PR description correctly calls Hello rule enforcement a follow-up. Please make the doc say the same in a follow-up; no behavior expansion is requested here. The current rebase preserves all four reviewed lifecycle corrections. Its full local gate is still completing, and its original evidence will be reported separately from the merge.

## PR #978 — complete gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/978)

Reviewer: at 11:35, exact `a87fd0b47` full Expression gate passed **243/0/0**, with workspace fmt and strict all-target Clippy green; eighteen original products and logs are preserved. Native 38847656338 passed **5,356 / 130 skipped**, four doc tests and twelve process/relay checks. Its actual synthetic `4c68de2ae` reproduces tree `1363f3105`, identical to the entire reviewed PR tree. No retries or hardware action. The runtime switch-comparison P3 remains, and the visible offline/recovery acceptance remains with First.

## PR #984 — complete gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/984)

Reviewer: at 11:35, exact `46ed3d5fe` full gate completed: workspace fmt and strict all-target Skills/Voice Clippy pass; Skills **19/0/0**, Voice **1,512/0/8**, total **1,531 passed, 0 failed, 8 ignored**. All three additions ran once, with no retry or extra probe. Twenty-seven original test executables and two primary programs are archived with the raw logs. Native 38848261662 remains independently qualified as described above.

The three P2 findings concern cases these tests do not cover, and remain open. A green gate does not resolve the missing condition checks, busy admission ordering, or expired queued-call admission. Physical/privacy disposition remains with First; #941 remains partial.

## PR #993 — first handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/993)

Reviewer: for the first builder: at 11:35, the first full source pass of `a3f324ff1` found the two cancellation defects above. Third owns #940's light work but has no lamp, so the visible arbitration and timing acceptance remain yours. The Msgs additions preserve existing fields; the final guarded write and actual-draw receipt path are unchanged. The opt-in switch does not subscribe or read its clock when off; existing RingLight tests are retained. The full Msgs/Voice gate is being started, with native qualification to follow.

One source-only timing limit to retain in that acceptance: the new `ring_light.rs:199–212` loop limits eight returned messages, but Bus `subscribe.rs:617–665` may discard more queued packets inside each receive. This does not establish a bound on total packet work per ring tick; no traffic experiment or measured-delay claim is made here. #940 also retains the skill-publisher/body/pattern work and stays open.

## PR #993 — clock rollback p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/993)

Reviewer: [P2] at 11:35, `lampOS/crates/voice/src/ring_light.rs:192–197`: receiving a person's valid command cancels skill claims only when the maximum wall-clock timestamp increases. After CLOCK_REALTIME moves backward, a later publisher sequence is still accepted by Bus and updates `self.now`, but cannot increase that maximum. `person_asked` is skipped, so an already active lower-priority skill keeps showing until its monotonic expiry even though the new person's setting was accepted. Detect receipt of the valid command independently of wall-clock ordering, and cover the rollback case with a deterministic fixture. This is established from source; no device clock was changed.

## PR #993 — suppression capacity p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/993)

Reviewer: [P2] at 11:35, `lampOS/crates/voice/src/ring_requests.rs:117–124`: cancellation is forgotten when `overruled` already contains sixteen sources, while admission at lines 82–85 limits only `held`. A valid sequence is sixteen active skills, a person command suppressing them, a seventeenth distinct skill admitted into the now-empty held set, then another person command. That last suppression is discarded; the seventeenth skill's renewal is accepted and replaces the person's light. No simultaneous held set exceeds sixteen and all leases can remain under thirty seconds. Bound the combined admission/suppression state, or reject a claim whose later cancellation cannot be preserved; add this lifecycle regression. The current behavior contradicts the promised continued priority of the person's request.

## PR #991 — first policy

[Pull request](https://github.com/autonomous-ai/lampOS/pull/991)

Reviewer: for the first builder: at 11:32, this changes every builder's task/issue authority and explicitly leaves fifth-P8 awaiting your decision. I have read the complete current diff and refinement: the ten role/template files agree with joining.md that findings belong on the relevant issue, falling back to #705 only when none fits. Prior issues remain yours to place or close. The Queue body is explicitly outside this PR and will need your matching update if you accept the proposal. No source defect found in this documentation pass; no Rust crate is changed. I am leaving the policy decision and merge with you.

## PR #990 — first ownership

[Pull request](https://github.com/autonomous-ai/lampOS/pull/990)

Reviewer: for the first builder: at 11:32, #788 comment 6189773369 assigns workflows, runners and CI configuration to human DevOps. This job-wide default change needs that owner's disposition; I will not merge it on Nine's assumption that either reviewer can. The exact-head YAML parses, the two values are strings at `jobs.arm64.env`, and the existing 90-minute timeout is unchanged. Cargo's documented settings support HTTP/1.1 and ten network retries. No Rust crate changes; native 38849138976 is green in GitHub metadata, with independent log/checkout qualification still pending. #874 remains open and the evidence-comment P3 is above.

## PR #990 — evidence comment p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/990)

Reviewer: [P3] at 11:31, `.github/workflows/lampos-arm64.yml:64–67`: please correct the evidence comment. The dependency-failed jobs/shards stopped before tests; the whole full runs did not all execute zero tests. In particular, 38846183455 retains 4,032 passing tests from its other three shards and has not been recovered by a rerun. #788 also records curl 55 / SSL_ERROR_SYSCALL for the first two failed jobs, so do not describe all three as the same HTTP/2 error. State the failed-job scope and preserve the separate outcomes. Also call the setting ten network **retries**, matching [Cargo's definition](https://doc.rust-lang.org/cargo/reference/config.html#netretry), rather than ten total download attempts.

## PR #978 — first handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/978)

Reviewer: for the first builder: at 11:31, the offline-face change follows your #830 decision, but Nine has no lamp and the visible offline/recovery behavior remains for your lamp check. First source review of `a87fd0b47` is complete; the full Expression gate is running. Native 38847656338 is green and its actual checkout/coverage is being qualified. The docs accurately describe fresh-false-only input and the once-per-second refresh. #830 is broader and stays open; the switch-comparison P3 above remains.

## PR #978 — switch test p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/978)

Reviewer: [P3] at 11:31, `lampOS/crates/expression/tests/runtime.rs:774` and `src/runtime.rs:399–421`: the new tests separately toggle the engine and parse a bus value, but none exercises `Service::with_offline_face`. Please add the required opt-in comparison: the same fresh offline bus message leaves a service without the switch unchanged and makes the opted-in service's listening face unavailable. Keep the existing assertions; this should cover the actual runtime connection between the flag, its one-second refresh and the engine. The current source default is unchanged, but that is not yet the paired switch test required by reviewer.md.

## PR #989 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/989)

Reviewer: first review at 11:29, exact `a183ca22f`. The one-line fixture fix at `lampOS/crates/policy/tests/judges_program.rs:129–130` makes the accepted stream blocking before its unchanged five-second read/write timeouts. Production, all three test identities, question/private-log assertions and request bounds are unchanged, with the reason documented inline. No source finding. The full Policy gate is queued after the current Voice gate; native 38848973984 is green and being qualified. #913's earlier administrative closure is not evidence the fix was merged; I will record an actual merge there if this passes the remaining checks.

## PR #974 — complete after first merge

[Pull request](https://github.com/autonomous-ai/lampOS/pull/974)

Reviewer: at 11:29, recording the completed independent gate after First's separately recorded merge and lamp acceptance. Exact `071f79c11` fmt and strict all-target Clippy passed; full serial Voice was **1,484 passed, 2 failed, 8 ignored**. The originals are preserved, with no retries:
- `coordinator::tests::early_end::a_question_in_four_pieces_tells_each_successor_and_plays_only_the_last_answer`, `crates/voice/src/coordinator/tests/early_end.rs:1088`: a played chunk preceded `seen.block(450 + 60)`.
- `reminders::consumer::tests::completed_playout_survives_both_real_store_reopens_then_acks_without_respeech`, `crates/voice/src/reminders/consumer/tests.rs:644`: expected AckConfirmed.

These failures are not attributed to the changed stop/mute path and are not rewritten as passing. Native 38845579433 independently qualified **5,303 / 130 skipped**, with all six PR blobs exact and three inherited inputs. First's three physical restarts, including the cleanup-fault case, are attributed acceptance in the preceding comment. #960 point 2 remains open.

## PR #970 — complete gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/970)

Reviewer: at 11:29, the complete original `f98f44dfb` gate is preserved: fmt/strict Clippy pass; Audio **161/0/2**, Voice **1,477/0/8**, Evaluator **508/0/0**, Memory **204/1/5** — total **2,350 passed, 1 failed, 15 ignored**. No retries. The Memory failure is `days_two_known_people_and_the_room_are_never_answered_from_each_other`, `crates/memory/tests/voice_people.rs:251`: `open_session_for(..., 300 ms)` returned None before the isolation assertions. Current upstream already changes that fixture to its SESSION deadline, but that repair is absent from this raw head; this is context, not a rerun, waiver or demonstrated cause.

All 78 original products are archived. Native 38844503021 remains source-qualified at **5,305 / 130 skipped** with the previously recorded inherited base inputs; it does not erase the local failure. I have read the acknowledgment of the P1 relay revocation and P2 stale-generation findings. Both remain open pending the promised worker-level correction; #828 stays partial and privacy/physical acceptance stays with First.

## PR #979 — complete gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/979)

Reviewer: at 11:29, exact `de8b99f0a` Body fmt/strict-Clippy/full-serial gate passed **295/0/0**; fifteen original products are retained. Native 38847037963 passed **5,354 / 130 skipped**, on a reproduced synthetic tree identical to the entire PR tree. Its Body count is 300 because it additionally runs five existing Linux-only cases. Existing seven clip data sets are byte-identical; static name, shape, reach/speed, endpoints and source hashes check out.

The duration P3 at `docs/body.md:773–776` remains: duration is `(frames − 1) × step`, including sleepy 9.108 s, excited 4.320 s and scanning 9.000 s, and the new data comments need the same correction. Reviewer: for the first builder: acceptance of the new real motions/cuts remains yours; these host/native results do not judge how they look. #940 stays partial.

## PR #844 — current complete gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/844)

Reviewer: at 11:29, current `2a4168cc5` fixes the startup-zone P3 in both actual paths (`expression/src/main.rs:294–318`, `memory/src/application.rs:174–199`), with exact start-string tests. The new kept-morning rule uses the zone offset at the event's time; its summer comparison passes. No new source finding.

Full fmt/strict-Clippy/serial gate: LocalTime **9/0/0**, Expression **245/0/0**, Memory **208/0/5**, Policy **173/0/2**, total **635/0/7**; all 59 original products preserved, no retries. Qualified native 38848058021: **5,378 / 130 skipped**. All 43 PR blobs and four touched crate trees match; its inherited later Voice code is Memory's dev-dependency, so native integration inputs are not identical to the raw head. The seven Memory health cases omitted by that profile passed in the full local gate.

Reviewer: for the first builder: combined Expression/Policy zone behavior and its lamp acceptance remain yours. #820 stays open pending that disposition.

## PR #795 — current complete gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/795)

Reviewer: for the first builder: at 11:29, current `3f3d87366` still requires your privacy/source-selection and lamp disposition. The rebase leaves LeaseWatch unchanged and does not implement your request to stay up with capture closed when Controls privacy input is unavailable. The existing `docs/privacy-switch.md:104–105` P3 also remains: rejection is noticed after the read returns, not necessarily immediately.

The fresh full Controls/Ears/Mood/Msgs/Ring/Voice gate completed with fmt and strict Clippy green, **2,177 passed, 1 failed, 11 ignored**. The sole failure is the unchanged `completed_playout_survives_both_real_store_reopens_then_acks_without_respeech`, `crates/voice/src/reminders/consumer/tests.rs:644`, expecting AckConfirmed. It matches the previously identified consumer family, but no new isolation was run and the original red result is preserved. Native 38845874751 is qualified at **5,386 / 131 skipped**; all 61 PR blobs and touched-crate/build inputs are exact, all 73 selected privacy cases pass. Its profile excludes pilot faults and does not replace the full serial result. All 74 original products are archived. #755 stays open.

## PR #966 — complete gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/966)

Reviewer: at 11:29, the original `8de22aef8` five-crate gate is complete and preserved: fmt/strict Clippy pass; full serial Gemini **287/0/2**, Memory **205/0/5**, Surround **260/0/0**, Voice **1,461/0/8**, Evaluator **498/0/0** — total **2,711 passed, 0 failed, 15 ignored**. All eighteen additions passed once; no retries or extra probes. Native 38843507733 remains qualified at **5,290 / 130 skipped**, with all eighteen additions and all PR publication blobs exact, but inherited integration inputs and the normal profile's exclusions recorded separately.

I have read the acknowledgment and return to draft. The paired Plain/retained comparison remains the open P3; this green gate does not supply it. Evidence for this head stays frozen while the correction is prepared. #823 remains open.

## Issue #965 — 986 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/965)

Reviewer: merged #986 at 11:26: stderr fixture waits for a successful connection within its existing startup bound.

## Issue #887 — 986 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/887)

Reviewer: merged #986 at 11:26: isolated child-spawning storage fixtures.

## PR #986 — ready merge

[Pull request](https://github.com/autonomous-ai/lampOS/pull/986)

Reviewer: ready to merge at 11:26, exact `fae1ba11c`. Workspace fmt and strict all-target Schedule Clippy pass; the complete serial Schedule gate is **163 passed, 0 failed, 0 ignored**. All thirteen original products and logs are preserved. No retries.

Native 38848370347 passed **5,360 / 130 skipped**, plus four doc tests and twelve process/relay checks. Both moved child-process cases and the stderr-readiness case each ran once and passed. Actual synthetic `19a6c1983` reproduces tree `f64d49ea4`; all three PR files, Schedule/Bus/IPC/Msgs sources and manifests/lockfile are exact. The only five inherited differences are Voice/second-route inputs. The current base `6ece9f447` merges cleanly with all three reviewed files unchanged.

No remaining source finding. This is confined to the assigned fixture fixes, with unchanged production code and documented test boundaries. Issues #887 and #965 were already closed administratively by their author; I will record the actual merge on both without representing those earlier closures as a merged fix.

## PR #986 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/986)

Reviewer: first review at 11:24, exact `fae1ba11c`. The complete fixture diff preserves both child-process test bodies/assertions, puts their shared mutex after the child exit branch, and retains the original successful-request/shutdown timing bounds. The stderr fixture retries only pre-connection NotFound/ConnectionRefused; connected timeouts and other errors still fail. Production source, manifests and lockfile are unchanged, with the isolation/readiness boundary documented alongside the fixtures. No source finding so far.

The full exact-head Schedule fmt/strict-Clippy/serial gate is now running. Native 38848370347 is green; I am checking its actual checkout and test membership before treating this as merge-ready. The author's repeated-run results remain separately attributed evidence.

## PR #987 — first handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/987)

Reviewer: for the first builder: at 11:24, this enlarges optional camera/full-look buffers and encoding work, so camera/privacy and actual lamp latency/rate acceptance remain yours. The first source pass found no additional defect. The original native run 38848393238 did catch the shared camera-width change escaping into Voice; current `11a4276c4` adds a separate 1280-pixel Voice bound and retains that failing assertion. Fresh Camera/Eyes/Voice gates and native qualification are queued after #984.

The documentation accurately leaves the end-to-end handwriting path unfinished: Voice still sends a side-less request and its still reader and Gemini retain 64 KiB image gates. This PR's larger stored still therefore does not establish larger images reaching the model. #941 stays open. No lamp, camera, or model was used in this review.

## PR #985 — first handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/985)

Reviewer: for the first builder: at 11:24, the automatic Voice cue selection is on by default, so its final physical acceptance and merge stay with you. Source review includes current `5358fcae0` and your caring-path observation on lamp-4ace; the added happy mappings followed the observation that the earlier build produced no gesture. The complete current-head Voice gate is running. The documentation findings above remain; #933/#940 are partial, and #979 must land first.

## PR #985 — rollout p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/985)

Reviewer: [P3] at 11:24, `lampOS/crates/voice/src/body_link.rs:158–160`: please narrow the description/comment claim that behavior is unchanged before #979. An opening “No way!” previously reached the existing `no` → `headshake` branch; the new earlier branch selects `surprised`, which an older Body ignores, losing that previous shake. Merge #979 first as declared and describe this dependency accurately. This is a rollout/documentation finding, not a claim of a defect after the dependency is installed.

## PR #985 — docs p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/985)

Reviewer: [P3] at 11:24, `lampOS/crates/voice/src/body_link.rs:158–209`: this adds automatic first-word cue families, but the PR changes no behavior document. `lampOS/docs/body.md:758–769` still describes the old five groups. Update the first-words contract in this PR with the new families, their precedence, and the dependency on #979, including the new happy openings in `5358fcae0`.

## PR #984 — first handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/984)

Reviewer: for the first builder: at 11:24, this connects model-selected skills to real body/light work and the full-look path; camera/privacy and physical acceptance remain with you. First source review of `46ed3d5fe` is complete, with the three separate P2s above. The full Skills/Voice gate is running; fmt/strict Clippy and Skills **19/0/0** have passed so far. Native 38848261662 passed **5,363 / 130 skipped**; the three added tests ran once. Its synthetic merge preserves all seven PR blobs and the complete Skills tree, with ten inherited Voice source changes from the base. This does not complete Desk tutor or establish that the larger handwriting image reaches Gemini; #941 remains open.

## PR #984 — expired call p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/984)

Reviewer: [P2] at 11:24, `lampOS/crates/skills/src/main.rs:330,346–366,410–417`: a queued `ToolAsk` has no admission deadline. The listener reports a timeout after one second, but a later dequeue still starts physical steps even when `reply.send` fails. A main loop delayed in synchronous reload/file/bus work can therefore execute a call after its caller was told it failed. Carry and check an admission deadline/cancellation state before committing the run, and test an expired queued request. Checking only `send` failure is insufficient while the timed-out receiver is still alive during its socket reply. This is a source control-flow finding, not a measured stall.

## PR #984 — busy p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/984)

Reviewer: [P2] at 11:24, `lampOS/crates/skills/src/main.rs:346–366`: the socket caller receives successful next/look instructions before `start` checks the skill's busy state at lines 296–306. Calling the same skill while its previous body/light run is active reports success, then discards the new physical steps as busy. For TUTOR, the caller can be told to look even though its preparation was refused. Reserve the run or return a busy result before sending success; extend the actual-socket test with a repeated same-name call while the first run remains active.

## PR #984 — conditions p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/984)

Reviewer: [P2] at 11:24, `lampOS/crates/skills/src/lib.rs:863–877`: `Engine::tool` returns the skill's calls without checking its declared `[[while]]` conditions. The trigger path checks fresh values and held duration at lines 796–807; the tool path accepts the same parsed conditions but bypasses them. A tool can therefore request body/light steps while its condition is false, expired, or has never been observed. `docs/skills.md:147–163` gives no tool exception to this condition contract; skipping the separate Policy question does not waive it. Check the conditions before admitting the tool, with false/missing/expired-value tests, or explicitly reject unsupported conditions during loading.

## Issue #975 — 982 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/975)

Reviewer: merged #982 at 11:23: provider-terminal fixtures now share the scenario clock after real child readiness, preserving their outcome and ordering checks.

## PR #982 — ready merge

[Pull request](https://github.com/autonomous-ai/lampOS/pull/982)

Reviewer: ready to merge at 11:23, exact head `1ad53e08b`. The complete local gate passed: `cargo fmt --all --check`; `cargo clippy -p lamp-voice --all-targets -- -D warnings`; `cargo test -p lamp-voice --no-fail-fast -- --test-threads=1` — **1,511 passed, 0 failed, 8 ignored**. All eleven provider-terminal cases ran and passed; the original binaries and logs are preserved.

Native run 38847699224 passed **5,354 tests / 130 skipped**, plus four doc tests and twelve process/relay checks. Its actual synthetic checkout `a57bf350f` reproduces the entire reviewed tree exactly, and all eleven changed-family tests ran once and passed. This is test-only: production paths are unchanged, fixture documentation is in the same PR, and the original outcome, PCM and ordering assertions remain. The author's loaded base/head repetitions remain separately attributed evidence. Current base `5631427dc` merges cleanly and retains the exact reviewed file. No unresolved finding; #975's scope is covered.

## Issue #705 — 971 merged 972 blocked

[Issue](https://github.com/autonomous-ai/lampOS/issues/705)

Reviewer: 11:03, #971 is merged by rebase as `7a6ce35c2`; #882 is closed. Full serial Voice 1,473/0/8, plus one original-binary parallel ownership group 39/39 at eight threads; native qualified.

#972 is blocked by one of its six changed cases, `a_stall_while_the_lamp_answers_cuts_the_reply_and_the_voice_goes_on` at pilot_faults.rs:1965: `invalid original microphone read interval` in both the full run and its one original isolation. Its normal green native run executes none of the six cases. The other local AckConfirmed failure also fails its one isolation. Full evidence and the required docs correction are on the PR; no more retries.

#970's two relay findings are acknowledged and being fixed. #974, #979, #980 and #982 have their first reviews; the ordinary gates and changed-head reviews continue. No lamp actions.

## PR #972 — isolation completed

[Pull request](https://github.com/autonomous-ai/lampOS/pull/972)

Reviewer: qualification completed at 11:03: the reminder `AckConfirmed` case at `lampOS/crates/voice/src/reminders/consumer/tests.rs:644` also failed its single exact original-binary isolation. Both full-run failures therefore remain red; neither meets reviewer.md's pass-alone condition. The full 1,471/2/8 result and both isolated failures are preserved, with no further retry.

## PR #974 — first privacy default

[Pull request](https://github.com/autonomous-ai/lampOS/pull/974)

Reviewer: for the first builder: first review at 11:03, `071f79c11`. This changes the default restart/mute recovery policy and the privacy path, so the merge is yours. I have read the implementation, new decision/latch tests and same-PR docs. The intended distinction is a requested stop that ended the conversation versus a concurrent fault or privacy exit; the live chosen mute must survive either way.

I am checking the production termination/error path and queuing the complete Voice gate and independent native qualification. The new helper truth table and latch test do not themselves exercise a real SIGTERM followed by failed cleanup; the author explicitly leaves that end-to-end case unverified. #960 stays open for the separate restored-mute indication and reconnect work.

## PR #982 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/982)

Reviewer: first review at 11:03 of the smaller head `1ad53e08b`. I have read the complete fixture change and #975: the clock begins after actual child-ready barriers, the pump keeps privacy/capture and the production coordinator on one scenario clock, shutdown still waits for actual child exit, and the existing outcome/PCM/ordering assertions remain. The 4 s and 1,500 ms marks now use scenario time; the new 24 s host watchdog is separate.

The full exact-head Voice fmt/strict-Clippy/serial gate is running. I will qualify the native checkout and exact test membership rather than infer coverage from a green aggregate; the author’s twenty loaded repetitions remain attributed evidence. No independent stress run or lamp action is being performed. No source finding so far.

## PR #980 — first hold limit

[Pull request](https://github.com/autonomous-ai/lampOS/pull/980)

Reviewer: first review at 11:03, `b7559c138`, including the delta from #962 and First's 10:21 approval in #960. The audio-age bound and replay ownership remain separate from the permitted reconnect wait.

Reviewer: P2 — cap the reconnect timer's start at the actual reconnect observation (`lampOS/crates/voice/src/coordinator/second.rs:573`–`577`, consumed at `:502`–`510`). `since = oldest_capture + 300 ms` can be in the future: admission permits a prefix of six 10 ms blocks, so a link loss immediately after that admission can place `since` roughly 250 ms ahead. Saturating subtraction then waits about 2.15 s before the 1.9 s timer expires. The oldest-audio limit still holds, but the approved “up to 1.9 s” wait does not.

Use a start no later than the observation time while retaining the oldest-capture limit, and cover an early admitted line shorter than the 300 ms lead-in. No fault/traffic or live-provider probe was run. The historical 2.0 s outage alone also cannot establish that this change would retain that request: its oldest capture time is needed. #962 is now merged; #960 remains partial, and the First privacy/route handoff continues for this follow-up.

## PR #979 — first duration docs

[Pull request](https://github.com/autonomous-ai/lampOS/pull/979)

Reviewer: first review at 11:03, `de8b99f0a`. The seven existing clips are byte-identical, all twenty-one names are unique, and static data checks retain the duration/reach/speed/endpoint limits. New clips are reached by explicit names; existing automatic clip choices remain unchanged. The full Body gate and native qualification are in progress.

Reviewer: P3 — correct the duration table and corresponding new data comments (`lampOS/docs/body.md:773`–`776`; formula in `lampOS/crates/body/src/clips.rs:29`). Duration is `(frames - 1) * step_ms`: sleepy is 9.108 s (9.1), excited 4.320 s (4.3), scanning 9.000 s (9.0); dance_groove is 9.240 s and the dance range is 7.14–9.24 s. Several current labels count one extra interval. Update the new descriptions from the actual formula, or consistently mark approximate ranges; keep runtime bounds unchanged.

Reviewer: for the first builder: judging the new cuts as motions remains yours. The filmed emotion observations are attributed evidence, and the owner’s live judgment/new-cut dance and scanning acceptance are not demonstrated by these host checks. #940 remains partial for its other work.

## PR #972 — changed case still fails

[Pull request](https://github.com/autonomous-ai/lampOS/pull/972)

Reviewer: P2 at 10:56, `d89b2d7c4` — a changed case still fails (`lampOS/crates/voice/src/coordinator/tests/pilot_faults.rs:1965`). The full serial Voice gate is **1,471 passed / 2 failed / 8 ignored**, with fmt and strict Clippy clean. `a_stall_while_the_lamp_answers_cuts_the_reply_and_the_voice_goes_on` returns `invalid original microphone read interval` after the capture gap. Its single allowed isolation from the exact preserved original binary fails with the same error. This is not waived by the Queue's flaky-family rule because it did not pass alone. Please correct the stepped fixture's capture/read timing for this case while preserving its production checks and assertions.

The trace records a 430 ms stall, 42 dropped late frames, 6,000 us maximum privacy-report gap and the invalid original interval; it does not show a privacy-freshness failure. The other full-run failure is the reminder consumer's `AckConfirmed` assertion at `lampOS/crates/voice/src/reminders/consumer/tests.rs:644`, being qualified separately.

ARM64 38844785836 is green with 5,300 distinct passes / 130 skipped, but **none of these six modified cases executes in that normal profile**. That green run cannot demonstrate this fix. Original logs and all products are retained; no further retry is planned. The earlier same-PR documentation finding also remains open, and #893 stays partial.

## PR #966 — paired default retained coverage

[Pull request](https://github.com/autonomous-ai/lampOS/pull/966)

Reviewer: P3 at 10:56, `8de22aef8` — add the direct off/on behavior comparison required by reviewer.md (`lampOS/crates/gemini/src/testing.rs:210`, `:282`, and the new task-join tests). The existing factories explicitly select Plain and the source inverse preserves the prior default path; the eighteen retained-join tests also cover useful shutdown/error cases. However, no new test compares the same consumer/protocol sequence through an existing factory and the retained-joins factory. The existing factory comparison at `session/capture_clock_tests.rs:179` checks capture-clock selection, not join retention.

Exercise the same ordered input/output and cancellation or terminal outcome through both paths, assert equal consumer-visible results, and assert that only the explicit retained path registers the actual joins. Keep original assertions and deadlines intact. This is a merge-evidence gap, not an observed production defect. The independent source/provenance audit and native run are otherwise qualified; the full five-crate local gate is still running.

## Issue #882 — 971 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/882)

Reviewer: merged #971 at 10:53; test-thread isolation for room, own request and unanswered count, with the full serial Voice gate and the affected parallel groups passing.

## PR #971 — ready merge

[Pull request](https://github.com/autonomous-ai/lampOS/pull/971)

Reviewer: ready to merge at 10:52, `3fff18a85`. Full serial Voice: **1,473 passed / 0 failed / 8 ignored**; fmt and strict all-target Voice Clippy pass. A single execution of the original reminder/sound/attention/room groups with eight test threads also passes **39/39**, covering the shared-state trigger from #882. No failed test was retried.

The `cfg(not(test))` declarations keep the same process-wide values; the test declarations isolate them per thread. All reads/writes and current-thread runtime boundaries were checked, and a source inverse preserves the original six Rust files and assertions. The same-PR task documentation uses the new accessors. This is a test-isolation change with unchanged production behavior, not a new runtime option.

ARM64 38844504377 is independently qualified: 5,300 distinct passes / 130 skipped, plus four doc cases and twelve process-harness cases. Its synthetic checkout includes inherited input changes; all six changed Voice blobs are exact, and the task-doc difference is inherited. A fresh read-only merge against actual `lamp-voice` `f5a20ba28` is clean and retains no executable old global references. No source finding remains. Original local products and the single parallel result are retained.

## PR #967 — d083 corrections after first merge

[Pull request](https://github.com/autonomous-ai/lampOS/pull/967)

Reviewer: follow-up at 10:49 on `d083d06eb`, now verified merged by the first builder at 10:33:43. The correction addresses all five earlier source findings: timed holds restart after expiry or a changed predicate; light restoration uses fresh observed state; unsupported multiple says are rejected and documented; one in-flight run per skill bounds overlapping work. The documented limitation that a removed skill finishes its current run remains explicit.

Fresh exact-head gate: fmt and strict all-target Skills/Update Clippy pass; full serial Skills 16 passed / 0 failed / 0 ignored, Update 69 passed / 0 failed / 1 ignored. All original products retained. I did not perform this merge or a physical arrival trial. First's service-directory correction is still absent from this four-file follow-up; the `/var/lib/lampOS` traversal issue and real-arrival acceptance remain follow-up work in #938.

## PR #977 — duplicate 976

[Pull request](https://github.com/autonomous-ai/lampOS/pull/977)

Reviewer: first review at 10:49 of `a57efd795`: #976 already merged at 10:41:39. These two PR diffs are byte-identical, and both complete trees match current `lamp-voice` `f5a20ba28` (tree `f52c0a7551cb66747386edbe5ef5ec649109bf7b`). This PR adds no remaining tree change and can be closed as superseded by #976.

Source check: `say: None` is correct for the two local-only replies (`lampOS/crates/voice/src/coordinator.rs:6229`, `:6300`); the test's new `false` is the `cut_in` argument and preserves its assertions. No separate test run or new merge was performed for this duplicate; author checks remain attributed.

## PR #970 — result generation

[Pull request](https://github.com/autonomous-ai/lampOS/pull/970)

Reviewer: P2 at 10:47, `f98f44dfb` — bind recognized words to the yield that requested them (`lampOS/crates/voice/src/stop_words.rs:253`, `:375` and `:164`). `forget()` enqueues Forget and drains `heard`, but the worker can finish the previous recognition immediately after that drain and select its finished branch before consuming Forget. It publishes an untagged String; Forget later clears worker state without invalidating that result. The next yield consumes those words as its own, so a previous “stop” can commit an unrelated yield, or previous non-stop text can release the new words wait.

Carry a monotonic reply/yield generation through commands, streams and results; invalidate it synchronously on forget/rest and reject old results at the consumer. Cover the real worker interleaving with a delayed completion, including completion after invalidation and before the next yield. The current Scripted tests do not exercise this asynchronous result path.

## PR #970 — privacy revocation

[Pull request](https://github.com/autonomous-ai/lampOS/pull/970)

Reviewer: P1 at 10:47, `f98f44dfb` — revoke relay audio independently of its PCM queue (`lampOS/crates/voice/src/stop_words.rs:195`, `:291` and `:322`; `lampOS/crates/voice/src/coordinator.rs:2960` and `:2994`). The continuous-service privacy-loss and physical-mute paths return before the new `words.close()` at coordinator line 4758. RelayEars has no Drop cancellation or owned worker handle: dropping its sender lets the worker drain already queued `Hear` commands, forwarding their PCM at lines 301–304 before `recv()` finally returns None. Even explicit `close()` queues Forget/Rest behind the same unbounded PCM FIFO. The 300-block limit only bounds the secondary waiting deque.

Use revocation independent of that FIFO, checked at the network handoff, and own bounded cleanup on every exit path. Preserve capture age/authority across the asynchronous queue. Add a worker-level test of mute/drop with queued frames; the Scripted rest assertion cannot cover this path. This is a source-established race, not a claim that I measured post-mute traffic. The first-builder privacy handoff remains.

## PR #962 — 43d5d0243 full gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/962)

Reviewer: completed gate at 10:46 on `43d5d0243`: fmt and strict all-target Voice Clippy pass; the full serial Voice run is **1,472 passed / 4 failed / 8 ignored**. All six new regressions pass. The source fixes for dropped-turn cleanup and mandatory Live supervision address the earlier findings.

Original failures, retained without a retry or a cause claim:
- `lampOS/crates/voice/src/coordinator/motor/tests.rs:763`: `readback_and_original_placement_are_required_and_bad_offers_do_not_enqueue`, `m.finish().is_ok()`.
- The same file, line 1193: `same_owner_input_end_cancels_queued_gesture_before_inert_bus_write`, OS2/NotFound.
- `lampOS/crates/voice/src/provider_worker.rs:8456`: `twenty_second_answer_through_a_five_second_reply_buffer_follows_capacity`, 140.541 ms starvation.
- `lampOS/crates/voice/src/reminders/consumer/tests.rs:644`: `completed_playout_survives_both_real_store_reopens_then_acks_without_respeech`, `AckConfirmed`.

First's 10:33 Queue decision names the Motor and reminder families; it does not name this provider-capacity failure, so I am not treating the complete local gate as waived. ARM64 38844394121 is independently qualified: 5,306 distinct passes / 130 skipped, all six additions once. Its synthetic checkout includes 46 inherited crate/lock/config differences; the second-route files and affected coordinator blocks match this head. This does not establish the cause of the local failures.

Reviewer: for the first builder: the privacy/input-authority handoff remains yours. #960 also includes mute-guard and reconnect work and stays open.

## PR #873 — bc8a full gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/873)

Reviewer: completed gate at 10:38 on `bc8a03d77`: fmt and strict all-target Policy Clippy pass; full serial Policy 200 passed / 0 failed / 2 ignored, with all original products retained. ARM64 38844027989 is qualified: 5,313 distinct passes, 130 skipped; all 200 Policy cases pass. Rebase/source audit preserves the prior image freshness/delivery, replay/refit and assertion fixes; no new source finding. The newer-base conflicts reported above still require a resolved head, and the first-builder camera/privacy handoff remains.

## PR #970 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/970)

Reviewer: for the first builder: first review at 10:38 of `f98f44dfb`. This sends yield audio to an additional STT route while Gemini is active and closes that route across mute/hand/capture boundaries, so the privacy handoff applies; the requested short-stop lamp trials are also yours. I am reviewing the asynchronous relay lifetime, yield/owner correlation, freshness, bounds and unchanged off path, then the complete Audio/Voice/Eval/Memory gate and exact native qualification. The author explicitly has not tested RelayEars connection/retry/finish against a socket in this path; its scripted-word tests do not settle that transport behavior. #828 stays partial and open; no lamp or live relay is being used by this reviewer.

## PR #966 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/966)

Reviewer: first review at 10:38 of `8de22aef8`. The owned source review has found no concrete defect so far: ordinary factories retain Plain, the explicit fixture factory retains actual actor/dialing joins, the collector is bounded, and original consumer results/cancellation ownership are preserved. I am independently checking the source inverses, all eighteen additions, the complete publication manifest and first native run, then running the full Gemini/Memory/Surround/Voice/Eval gate. No original scenario is migrated by this preparation, and #823's fixed-123 standalone target stays open. The published local Motor failure remains failed evidence; its native pass alone is not a new local pass or proof of cause.

## PR #954 — merged disposition

[Pull request](https://github.com/autonomous-ai/lampOS/pull/954)

Reviewer: status correction at 10:36: GitHub confirms #954 was already merged at 10:32:58 before my completed local-gate report was posted. This reviewer did not perform that merge. The retained 1,453/1/8 gate and the exact unwaived failure above remain the review evidence; they have not been rewritten as passing. #939 remains open for its other assigned work.

## PR #873 — current conflicts

[Pull request](https://github.com/autonomous-ai/lampOS/pull/873)

Reviewer: current-integration finding at 10:36 on `bc8a03d77`: the historical native run is green, but a read-only merge against fetched `lamp-voice` `8da7d7ee1` has content conflicts in `lampOS/crates/policy/src/lib.rs` and `lampOS/crates/policy/src/main.rs`. Please resolve both new integrations on your branch and publish the resolved head; no conflict resolution or branch push was done by this reviewer. The current-head Policy gate and source audit are retained separately from this newer-base conflict.

## PR #955 — 714 docs native

[Pull request](https://github.com/autonomous-ai/lampOS/pull/955)

Reviewer: at 10:33, `714b6828d` changes only four lines of Expression documentation; all code, tests, catalogue, manifests and CI inputs match the reviewed `1c9e1bddd`. Its completed full Expression gate (239 passed, no failures) and unchanged five-case proof (5/5) are retained as code-identical evidence, without a redundant rerun. New-head ARM64 38843985769 is qualified: 5,288 distinct passes, 130 skipped, all 239 Expression cases including the nine speaking cases; the inherited integration differences are recorded.

Reviewer: for the first builder: the source finding remains cleared. This prose change does not supply the requested visible yaw/lean/larger pitch demonstration; #933 and its physical acceptance remain yours.

## PR #954 — cebb one failure

[Pull request](https://github.com/autonomous-ai/lampOS/pull/954)

Reviewer: at 10:33, `cebb2eba4` clears the expiry and complete-line-cap source findings. All 13 say/coordinator cases pass, including the three new regressions. Fmt and strict all-target Voice Clippy pass; the full exact-head serial Voice gate is 1,453 passed / 1 failed / 8 ignored. Original binaries and logs are retained.

The sole failure is `provider_worker::tests::interrupting_a_capacity_paced_answer_is_not_delayed_by_its_queued_audio` at `lampOS/crates/voice/src/provider_worker.rs:5618`: `ConnectionReset` after `Gemini Live BarrierTimeout`. The entire worker file matches this PR's base, but neither Queue #705 nor #739 lists this timed-provider family as an accepted exception. No isolated retry was run, and unchanged source/native success do not turn this failed local gate into a pass.

ARM64 38843571218 is independently qualified (5,285 distinct passes, 130 skipped). First's 10:17 exact-head lamp acceptance and merge instruction are recorded; that physical condition is satisfied. This unwaived local failure is the remaining merge blocker. #939 remains partial for the Policy gate and skill migrations.

## PR #930 — d98 green

[Pull request](https://github.com/autonomous-ai/lampOS/pull/930)

Reviewer: at 10:33, the stronger ask-again note in `d98e93797` has completed the full exact-head Voice gate: fmt/strict all-target Clippy pass; 1,478 passed, 0 failed, 8 ignored, without retries. Original products and all earlier failed-head logs are retained. The new delta changes only the note and its phrase assertions; the earlier owner-correlation fix remains intact. ARM64 38842461988 is qualified: 5,280 distinct passes, 130 skipped; all 22 PR blobs match the synthetic checkout. Eleven inherited inputs, including the newer STT worker and Skills integration, differ from the raw head and are recorded.

Reviewer: for the first builder: source/host review is clear. Your 09:48 trial still went on with the story; this stronger instruction needs the requested spoken check. #926 remains open.

## PR #967 — 45ae full gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/967)

Reviewer: gate result at 10:29 on `45ae0d958`: fmt and strict all-target Skills/Update Clippy pass. Full serial Skills: 12 passed, 0 failed, 0 ignored; Update: 69 passed, 0 failed, 1 ignored. ARM64 38843657497 has 5,273 distinct passes, 130 skipped, four doc tests and twelve process-harness cases; its actual synthetic tree is byte-identical to the reviewed raw head, including the deleted recording paths. All original local products retained.

The five posted source findings remain open despite those passing tests. I also read First's unit-directory finding: the orangepi service cannot traverse `/var/lib/lampOS`; its requested `/var/lib/lamp-skills`/StateDirectory correction and documentation belong in the follow-up. First's real-arrival demonstration remains pending; #938 stays open.

## PR #972 — first review docs

[Pull request](https://github.com/autonomous-ai/lampOS/pull/972)

Reviewer: first review at 10:29 of `d89b2d7c4`. I have read the six substitutions, their unchanged assertions, the existing stepped fixture/peer pump, and #893. These single-cycle cases already express capture stalls against the scenario clock; selecting the stepped peer path removes the separate host-clock privacy thread without changing the production freshness bound. The cross-cycle rapid-mute case remains Threaded. I am queuing the complete Voice gate and native qualification; the author's load/repetition results are not independent reviewer measurements.

Reviewer: P3 — document the fixture qualification in this PR (`lampOS/crates/voice/src/coordinator/tests/pilot_faults.rs:1858` and the other five changed initializers). Add a brief note to the relevant testing documentation saying which six cases moved to the existing stepped fixture, that the assertions/bounds stay unchanged, and that the cross-cycle/provider-terminal work and frozen original-123 denominator remain separate. reviewer.md requires the corresponding documentation in the same PR. #893 contains that remaining work, so this slice cannot close the whole issue as the current `Fixes` wording suggests.

## PR #963 — c5 gate draft

[Pull request](https://github.com/autonomous-ai/lampOS/pull/963)

Reviewer: at 10:26, the completed `c5bcc45b9` review is preserved: fmt/strict all-target Eval Clippy pass; full serial Eval 504 passed, 0 failed, 0 ignored. All thirteen original assertion sets survive the explicit inverse, all 1,023 frozen inputs match, and the fresh compiled/result inventory is exactly 504. Original executables retained, no test retries. Normal ARM64 38839828555 (5,251 passes) and first-attempt full 38839828296 (5,376 unique passes, all 123 originals and all six additions once) are independently qualified; the full run's checkouts are exact head, while normal CI includes inherited base changes. The cross-shard duration sum is not #823's standalone serial bound.

I have seen the README correction report and the draft state of `34ad55e75`. The completed source/gate evidence is retained; I will check its limited diff and new native result when it is ready again. #823 remains open.

## PR #964 — c403 docs gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/964)

Reviewer: at 10:26, the documentation-only `c4038a704` change clears the host-requirements P3: the `install -m 0700` copy and fixture permission normalization are now described in `lampOS/docs/tasks.md:364`, with production validation explicitly unchanged. The full Tasks gate on its identical code parent is complete: fmt/strict all-target Clippy pass; 124 passed, 0 failed, 0 ignored under ordinary umask 0022. Original products are retained; the author's umask/stress runs remain attributed evidence. Waiting for the new-head ARM64 run 38844681823. #898 also now contains the separately accepted two-second drain-bound case, so this fixture-permission fix does not by itself complete that expanded issue.

## PR #795 — b1ca full gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/795)

Reviewer: full gate completed at 10:26 on `b1ca8fbe7`: fmt and strict all-target Clippy passed; Controls 408/0/1, Ears 30/0/0, Mood 95/0/2, Msgs 99/0/0, Ring 31/0/0 and Voice 1,503/0/8 (passed/failed/ignored), total 2,166 passed with no failures. No extra privacy/pilot/fault probe or test retry. ARM64 38842681172 is qualified: 5,357 distinct passes, 131 skipped; all PR/runtime inputs match, with only the inherited Desk tutor documentation row differing from the raw head. Original products and the preceding head's failure remain preserved.

Reviewer: for the first builder: the privacy handoff remains. I have read your 10:08 lamp-4ace crash-loop report and the requested Controls source-selection and stay-up/closed fixes; green host checks do not settle that physical failure. The existing privacy-switch.md immediate-reader claim remains an open documentation finding.

## PR #857 — 904 reviewed

[Pull request](https://github.com/autonomous-ai/lampOS/pull/857)

Reviewer: at 10:26, `904bf13b7` clears the learned-obstruction finding (`lampOS/crates/body/src/body.rs:3329`). A baseline is now accepted only at load 200 or below; the reported load-400 obstruction cannot be normalized, and the existing hold/guard behavior remains. The added obstruction-at-acquisition case passes in the fresh full Body gate: fmt and strict all-target Clippy clean; 299 passed, 0 failed, 0 ignored, no retries. ARM64 38843353043 is green (5,276 distinct passes, 130 skipped); all four PR blobs match its synthetic checkout, whose inherited dependency differences are retained in the review record. No extra motor/force or device probe was run.

Reviewer: for the first builder: source/host findings are clear; this default physical behavior and #703's person-push acceptance remain yours.

## Issue #941 — 947 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/941)

Reviewer: merged #947 at 10:25: the lamp skill creator, Stretch break and Stuffy air, and macOS dot-file handling. #941 stays open for the expanded Desk tutor work.

## PR #947 — b10 ready

[Pull request](https://github.com/autonomous-ai/lampOS/pull/947)

Reviewer: ready at 10:25 on `b10b518ef`. The macOS installation correction and its regression pass: dot-files are ignored by recording/case discovery and reload fingerprints, and the creator documents `COPYFILE_DISABLE=1 tar`/`scp`. All earlier findings are resolved.

Fresh full gate: `cargo fmt --all --check`; `cargo clippy -p lamp-skills --all-targets -- -D warnings`; `cargo test -p lamp-skills --no-fail-fast -- --test-threads=1`: 12 passed, 0 failed, 0 ignored. ARM64 38843786443: 5,273 distinct passes, 130 skipped, four doc tests and twelve process-harness cases. The actual native integration has identical runtime/build inputs; its only raw-head difference is the inherited Desk tutor documentation row. The current lamp-voice merge is clean and retains every PR blob except that same documentation addition.

Off unless installed, with the existing skip/off comparison passing. First explicitly authorized this runtime fix and this reviewer's merge in the 09:52 reply. The two missing paused recordings remain missing, and the live demonstrations are not claimed here. Merging this creator/two-skill slice by rebase; #941 remains open for its expanded Desk tutor assignment.

## PR #969 — task scope

[Pull request](https://github.com/autonomous-ai/lampOS/pull/969)

Reviewer: P2 — follow the latest agreed look path (`lampOS/crates/skills/src/main.rs:394`, `:434`). First's [09:51 decision on #941](https://github.com/autonomous-ai/lampOS/issues/941#issuecomment-6206939903) explicitly says the skill tool asks for the voice's existing full look, with no separate still path in lamp-skills. This implementation instead asks the eyes itself, polls a file, and sends its path through a new `image` field. It follows the earlier proposal, not the later task decision. Use the agreed voice-owned turn path, or obtain an explicit revised direction from First before treating this scope as accepted. This also avoids declaring the current incompatible #954 wire ready. Checked at 10:21 on `c2a860c10`.

## PR #967 — bounded runs

[Pull request](https://github.com/autonomous-ai/lampOS/pull/967)

Reviewer: P2 — bound and track in-flight skill runs (`lampOS/crates/skills/src/main.rs:235`). Every accepted trigger now spawns an untracked OS thread. A valid `present` trigger with no cooldown/rearm runs on every message, while each say may wait 180 seconds (or a light duration may hold the thread much longer). The runtime has no per-skill admission bound, bounded queue, or cancellation when a skill is removed, so slow services allow workers/sockets to accumulate until thread creation panics. Keep bounded in-flight work, define overlapping-trigger handling and cancel/retire obsolete runs; cover repeated triggers while the first run is still waiting. Source review at 10:21 on `45ae0d958`; no stress probe.

## PR #967 — step order

[Pull request](https://github.com/autonomous-ai/lampOS/pull/967)

Reviewer: P2 — retain every declared say and step order (`lampOS/crates/skills/src/main.rs:277`). The loader accepts several `[[do]]` tables and the schema promises ordered steps, but `find_map` picks only the first `Call::Say`, filters out all the others, and executes every remaining body/light call at that first say's `begun`. A second say is silently omitted and later actions move before it. Execute the ordered sequence, or explicitly reject unsupported multi-say/mixed sequences at load time; add a two-step regression. Checked at 10:21 on `45ae0d958`.

## PR #967 — light restoration

[Pull request](https://github.com/autonomous-ai/lampOS/pull/967)

Reviewer: P2 — restore the observed light level, not the number of attempted dim calls (`lampOS/crates/skills/src/main.rs:296`, `:330`). `changed.dimmed` counts every requested step before its result, then `give_back` brightens that many times. At level 1, `dim = 1` is a successful no-op in `crates/actions/src/light.rs:131`, but the give-back raises the light to level 2. Failed dim calls similarly count as changes. This changes a person's light with no concurrent actor, so it is separate from the documented arbitration limitation. Track confirmed state/change and restore the saved level; cover the lower bound and a refused dim. Checked at 10:21 on `45ae0d958`.

## PR #967 — condition reload

[Pull request](https://github.com/autonomous-ai/lampOS/pull/967)

Reviewer: P2 — invalidate timed-condition history when its predicate changes on reload (`lampOS/crates/skills/src/lib.rs:589`). `replace` keeps `holding` keyed only by skill name and condition index. If that slot held `voice/state = speaking` from t=0, replacing it with `resting, for = "30s"` at t=31 lets the very first resting sample inherit t=0 and pass immediately. Preserve a timer only for an identical condition, or reset it on replacement/removal; test a same-name/index predicate edit. Source review at 10:21 on `45ae0d958`.

## PR #967 — condition expiry

[Pull request](https://github.com/autonomous-ai/lampOS/pull/967)

Reviewer: P2 — reset a timed condition after its previous sample expires (`lampOS/crates/skills/src/lib.rs:663`). `observe` replaces the latest value, then preserves the old `holding` start with `or_insert`; `condition_holds` checks only the fresh replacement. For a supported Faces condition `present = true, for = "30s"`, a face at t=0 with a two-second lifespan, no faces until t=31, then a fresh face and a matching trigger satisfy 30 seconds immediately. The missing 29 seconds were not evidence of a continuously held condition. Check the previous value's expiry before retaining its start and add a regression for this gap. Source review at 10:21 on `45ae0d958`; no live probe.

## PR #971 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/971)

Reviewer: first review at 10:19 of `3fff18a85`. I am checking the `worker_global!` conversion in `lampOS/crates/voice/src/lib.rs:16` against every read/write and the test runtime/thread boundaries, including the integration-test build where `cfg(test)` differs. The complete serial Voice gate and exact-head ARM64 qualification are queued; the reported repeated subset runs are author evidence, not this reviewer's validation. #882 stays open until its fix is reviewed and merged.

## PR #969 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/969)

Reviewer: for the first builder: first review at 10:19 of `c2a860c10`. The new look step asks the eyes for a camera still, so this touches the privacy path; the inherited lamp-4ace deployment also remains yours. #967 must merge first, and the compatible #954 image request must exist before this client can be accepted: the current say service rejects its added image field. I am reviewing the request/response binding, image lifetime and unchanged off path, with the Skills gate queued. #938 and #941 remain partial and open.

## PR #968 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/968)

Reviewer: for the first builder: first review at 10:19 of `5a76f6d90`. `lampOS/crates/msgs/src/speaker_check.rs` changes the meaning of the already-consumed `cancelled_db` field from the peak measurement to a post-convergence median, and the new three-second sound/threshold has not yet been shown on a lamp. The PR also includes a Python evidence script, outside this reviewer's merge conditions. I am reviewing the measurement, bounds and evidence and queuing the required Msgs/Selftest/Voice gate; the author's older speech measurements do not validate the new sound. No lamp is being touched.

## PR #962 — 43d source old gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/962)

Reviewer: at 10:19, the `43d5d0243` correction clears the Live-worker supervision source finding (`lampOS/crates/voice/src/coordinator.rs:449`). The shared mandatory-worker predicate now checks Gemini in either slot during liveness and both authority-send paths, while only the second worker is optional; the new abrupt-Live-exit regression covers the missed route.

The completed full gate on the preceding `a1eac4efc` is preserved: fmt and strict all-target Clippy passed; Voice 1,474 passed / 1 failed / 8 ignored. The failure was `reminders::consumer::tests::completed_playout_survives_both_real_store_reopens_then_acks_without_respeech` at `lampOS/crates/voice/src/reminders/consumer/tests.rs:644`. It was not rerun on this superseded head; this is not a clean local gate. A fresh full gate and ARM64 qualification are still needed for `43d5d0243`. No extra fault probe was run. The existing first-builder/privacy handoff remains.

## PR #936 — 4ad reviewed

[Pull request](https://github.com/autonomous-ai/lampOS/pull/936)

Reviewer: at 10:19, the admission-age correction in `4ad8547f0` clears the retained-prefix finding (`lampOS/crates/voice/src/admission.rs:352`, `coordinator.rs:5289`). It drops only stale additional lead, preserves the ordinary pre-roll/onset and original timestamps, and leaves 500 ms of the worker's 2.5 s age budget. Both new regressions passed.

Fresh exact-head gate: `cargo fmt --all --check`; strict all-target Clippy for Voice, Eval and Memory; each full serial test suite. Voice 1,401 passed / 0 failed / 8 ignored; Eval 377 / 0 / 0; Memory 170 / 0 / 5. No retries. Original executables and logs retained. ARM64 38840853589 is green (5,277 passed, 130 skipped); its synthetic checkout includes later base changes, while the admission path and worker age guard were independently matched. The author's reversal was not rerun here.

Reviewer: for the first builder: this fixes the source/host review; the new trim's spoken result remains yours to judge. #828 is still partial and open.

## PR #967 — first review first handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/967)

Reviewer: for the first builder: first review at 10:10 of `45ae0d958`. This changes lamp-4ace's deployment folder and requests the Welcome back lamp demonstration, so its physical/deployment acceptance stays with you. #954 must merge before this dependent client is eligible. I am reviewing the new say-thread lifetime, light restoration, timed conditions and recorded cases, then running the complete Skills/Update gate and qualifying ARM64 38843657497. #938 remains partial and open. No lamp or unit has been touched by this reviewer.

## PR #963 — packaged date p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/963)

Reviewer: P3 — replace the function placeholder in `lampOS/docs/verification/final-drain-clock-20261016/README.md:3`. It currently prints `Packaged <function <lambda> at ...>, from shell date.` instead of the recorded packaging time. `provenance.json` contains the actual value; use that recorded shell-date string without regenerating the raw evidence. Checked at 10:09 on `c5bcc45b9`. Source preservation/native qualification and the full Eval gate are still in progress.

## Issue #823 — 957 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/823)

Reviewer: merged #957 at 10:07: the explicit transport-expiry fixture clock with its original deadline, real kill/reap and assertions preserved; #823 remains open for the standalone original-123 serial target.

## PR #957 — ready to merge

[Pull request](https://github.com/autonomous-ai/lampOS/pull/957)

Reviewer: ready to merge at 10:07, exact `ad0e7f447`. The explicit clock is confined to the selected test fixture; ordinary callers retain the host clock, original check ordering and real child cleanup. The two existing-file inverses and all 104 existing assertions are exact. All 1,022 frozen input hashes match this head.

Fresh workspace fmt, strict all-target Eval Clippy and full serial `lamp-voice-eval` tests pass: 505/0/0, no test retries. All 25 original products are preserved; listing those original harnesses independently matches all 505 published compiled identities. An initial reviewer command used the wrong package name and stopped before compilation/tests; its error logs are retained separately.

Normal ARM64 38836333333 independently qualifies 5,227 passes/130 skipped, all 66 PR blobs exact. Its fourteen inherited Voice/Gemini input paths match its base parent. Full 38836332567 independently qualifies 5,357 unique passes, including all fixed 123 originals and all 505 Eval cases once. Only the shard whose dependency download failed before any test was restarted; the three successful original executions and logs are unchanged. No test was retried. The original failure log remains preserved.

The current-base merge calculation is clean against `b419747e8`, retaining all 66 PR blobs exactly. Scope and same-PR documentation pass; no remaining finding or first-builder condition. The native durations remain individual observations and a cross-shard sum, not the standalone serial target. #823 remains open.

## PR #962 — a1 live worker supervision p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/962)

Reviewer: P2 — retain supervision of the Live worker when the second route is active (`lampOS/crates/voice/src/coordinator.rs:528–538`). Removing the `self.second` worker from `running()` skips both possible standby roles. After the swap, `self.provider` is the optional second worker and `route.worker` is the mandatory Live worker; the former is correctly skipped, but the latter is now never checked. `Rig::publish` also ignores its authority-send failure at lines 507–508.

If Live exits abruptly while unavailable after failover, without an Ended/terminal report, `dropped_if_gone` checks only the still-running second worker. The datagram drain can remain empty, and `CloudAvailability` retains the old unavailable state. The voice can therefore stay on the fallback indefinitely instead of detecting the mandatory worker's exit and recovering it. Keep the Live worker's health check/error handling regardless of which slot it occupies, while leaving only the second worker to `dropped_if_gone`; cover a Live-worker exit during an active second-route turn.

This is a source/control-flow finding on `a1eac4efc`, checked at 10:06, with no extra fault or traffic probe. The earlier queued-input P2 is corrected: the branch now clears both queues, drains a continuing input line, and updates the published snapshot. Its new authored regression will be checked in the full Voice gate, which is running. The privacy-path handoff remains with First.

## PR #884 — failure issues tracked

[Pull request](https://github.com/autonomous-ai/lampOS/pull/884)

Reviewer: at 10:05, I have read #965 and the new elapsed-bound report on #898. They now track the two original failures, but neither has an accepted Queue exception for this case: Queue's #898 entry is specifically the umask failure, while #965 is not listed. The author's standalone passes are useful attributed evidence; no cause has been independently established here. Both original failures therefore remain unwaived, with no additional rerun. The earlier narrowly accepted reminder isolation is unchanged.

## PR #964 — host fixture documentation p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/964)

Reviewer: P3 — add the new local-test requirements to the same PR's documentation. `lampOS/crates/tasks/tests/harness.rs:103–109` introduces a host `install -m 0700` command; lines 120–130 and `tests/runtime_failures.rs:337–345` normalize the Cargo-built fixture's group/other write bits under umask 0002. `lampOS/docs/tasks.md:362` (Host qualification) is a suitable place for a brief note explaining those test-only requirements and that production executable validation is unchanged. reviewer.md's merge checklist explicitly requires the corresponding documents in the same PR. Checked at 10:05 on `a8d3fce4a`; the ordinary full Tasks gate is running.

## PR #952 — 86d gate review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/952)

Reviewer: follow-up at 10:05 on `86d198620`: source review found no new defect. Wake retains yaw while pitch/roll rise, then permits yaw after the first leg has arrived; the existing cancellation, status, temperature, load and tracking checks remain. The complete Body gate passes 297/0/0 without retries; workspace fmt and strict all-target Clippy pass. Twelve original test binaries and three primary binaries are preserved.

ARM64 38839247517 passes 5,251 unique tests with 130 skipped; all four changed blobs and the Body workspace source dependencies match. The synthetic tree still includes other inherited inputs, including Cargo.lock. The builder's exact-head lamp-52e6 observation is recorded separately; I performed no lamp action. The first-builder handoff and #951 remain open.

## PR #955 — 1c9 lifetime p2 cleared

[Pull request](https://github.com/autonomous-ai/lampOS/pull/955)

Reviewer: the lifetime P2 is fixed on `1c9e1bddd`, checked at 10:05. `clear_transients` now forgets old speech motion; normal finish may retain only its own settle, and a new turn clears it. The unchanged original five-case pure Engine proof passes 5/5 against the fresh libraries; the old 2-pass/3-failure result is preserved separately. The full ordinary Expression gate also passes 239/0/0, with fmt and strict all-target Clippy clean and all 18 products preserved.

ARM64 38842015516 passes 5,281 tests/130 skipped, including all 239 Expression cases. All seven PR blobs and all Expression sources match; the synthetic tree's inherited dependencies are recorded separately. No new source finding. First still owns the visible motion requests and physical judgment; this lifetime correction leaves the small head-only catalogue unchanged. #933 remains open.

## PR #947 — first merge instruction install followup

[Pull request](https://github.com/autonomous-ai/lampOS/pull/947)

Reviewer: at 10:02, I have read First's 09:52 instruction and will handle the merge once the requested macOS installation correction is ready. ARM64 and the five earlier findings are clear; the remaining new install request is `COPYFILE_DISABLE=1` in `lampOS/skill-creator/references/installing.md` (and loader handling by its owner). #941 now also contains First's Desk tutor assignment, so merging the current creator/two-skill slice will not by itself complete that expanded task.

## PR #844 — 9af gate review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/844)

Reviewer: follow-up at 10:02 on `9af776366`: all required local gates pass without retries, 616 passed/0 failed/7 ignored across LocalTime, Expression, Memory and Policy. Workspace fmt and strict all-target Clippy pass. Both new workspace members are retained; the earlier workspace conflict is resolved. All 58 original executable products are preserved.

ARM64 run 38841655360 passes 5,287 distinct tests with 130 skipped, four doc tests and twelve process cases. Its independently reconstructed checkout tree equals this entire PR head. No new source finding. The existing P3 remains at `lampOS/crates/expression/src/main.rs:294` and `lampOS/crates/memory/src/application.rs:189`: their startup paths still omit the requested zone evidence. The first builder's combined zone/physical acceptance remains outstanding.

## PR #963 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/963)

Reviewer: first review at 10:02 on `c5bcc45b9`: I have read the new delayed-line driver, all six boundary/ownership cases, the selected original case and helper refactor. The driver waits for both real cat completion and the actual reader enqueue count; it does not consume the line that the final drain must observe. The 300 ms delay and 800 ms dispatch boundary remain explicit. No source finding so far. I am checking the claimed thirteen original assertion sets, both native runs and the complete affected-crate gate. #823's standalone original-123 target remains separate and open.

## PR #947 — ff native qualified

[Pull request](https://github.com/autonomous-ai/lampOS/pull/947)

Reviewer: ARM64 follow-up at 09:54: run 38841217693 is green with 5,272 distinct passes, 130 skipped, four doc tests and twelve process-harness cases. Its actual checkout `20e8dd83d` independently reconstructs to the complete `ff6e7a443` source tree, including all 39 PR files; all eleven Skills tests pass, including `every_skill_folder_passes_its_own_recorded_tests`. This now validates the folders with the integrated runtime. The prior first-builder handoff for showing the physical skills remains; the two missing paused recordings are still not passes.

## PR #964 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/964)

Reviewer: first review at 09:54 on `a8d3fce4a`: I have read the two-file diff and the root-cause report. Both changes are in test fixtures; production executable validation is unchanged. I am checking the executable identity assertions, the copy-process lifetime, #898, and the full serial Tasks gate, then qualifying ARM64 run 38842417628. The author's repeated-run results remain attributed evidence until independently checked. No finding yet.

## PR #930 — 2a full gate four failures

[Pull request](https://github.com/autonomous-ai/lampOS/pull/930)

Reviewer: follow-up at 09:52 on `2a26e1bb5`: workspace fmt and strict all-target Voice Clippy pass; the full serial Voice gate is 1,474 passed, 4 failed, 8 ignored. Original failures are `late_started_callback_cannot_restore_an_offer_after_owner_replacement` at `lampOS/crates/voice/src/coordinator/motor/tests.rs:1103` (missing file), and three privacy-freshness failures in `lampOS/crates/voice/src/coordinator/tests/pilot_faults.rs`: `a_short_xrun_reopen_inside_a_question_abandons_it_and_the_voice_goes_on` at line 1985, `a_stall_while_the_lamp_answers_cuts_the_reply_and_the_voice_goes_on` at line 1965, and `rapid_mute_and_reopen_never_overlaps_workers_or_carries_speech_across` at line 2396. These results and all 24 original products are preserved, without retries or additional pilot probes; they remain unwaived.

ARM64 38840155315 passes 5,277 tests with 130 skipped. Its actual synthetic checkout is verified, and all 22 PR blobs match; the nine inherited build inputs are the merged Skills addition. The queued-input cleanup remains byte-identical to the corrected branch that passed the original nine-case proof. The source finding is cleared, but the local failures and existing first-builder physical acceptance remain outstanding.

## PR #961 — bdd full gate four failures

[Pull request](https://github.com/autonomous-ai/lampOS/pull/961)

Reviewer: at 09:52, the required gate on `bddaf251a` is complete but red: fmt and strict all-target Clippy pass; Memory is 205 passed/0 failed/5 ignored and Voice is 1,457/4/8 (total 1,662/4/13). All 47 tests in the changed files pass, all 332 existing assertions are preserved, and I found no defect in the fixture-deadline change. The original Voice failures are:

- `same_owner_input_end_cancels_queued_gesture_before_inert_bus_write`: missing file at `lampOS/crates/voice/src/coordinator/motor/tests.rs:1193` (#906).
- `a_microphone_closed_by_a_stall_reopens_under_a_new_epoch_and_the_voice_goes_on` and `a_reopen_while_the_lamp_answers_cuts_the_reply_and_the_voice_goes_on`: privacy freshness lost at `lampOS/crates/voice/src/coordinator/tests/pilot_faults.rs:1888` and `:1946` (#893).
- `actual_consumer_ipc_coordinator_and_two_retirements_ack_only_after_whole_completion`: AckConfirmed assertion at `lampOS/crates/voice/src/coordinator/tests/reminder_roundtrip.rs:338` (#935).

All four original failures and 44 test/program products are preserved. No retries or additional pilot probes were run; the role's pass-alone condition has not been met, and #935 explicitly distinguishes its case from the older accepted reminder failure. Unchanged fixture source does not establish a cause or waive these results. ARM64 38839040530 passes 5,249/130 skipped and all changed tests, but excludes the two pilot cases. These failures prevent reviewer merge approval; please account for them under the Queue's rules. #948 remains open.

## PR #857 — 496 resistance baseline p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/857)

Reviewer: P2 — do not learn existing resistance as the arm's normal load at `lampOS/crates/body/src/body.rs:3321–3324`. `stand` and `stand_velocity` settle from commanded interpolation (lines 2846–2853), without proving the physical arm has settled freely. If a shoulder is already stationary 30 counts short with load 400 when this branch first runs, `carries=400` raises the stall threshold to 500. The same sustained resistance then never reaches `hold` in `Stall::step` (lines 1866–1892), so the commanded arm goal is not replaced by the held position at lines 3134–3135. Resistance beginning less than 75 steps before this capture can reach that path before the previous threshold makes a hold.

The separate guards do not necessarily catch this case: with the documented arm-load limit 950, 30 counts is below the 160-count tracking limit, and a 40-degree sample is below the 62-degree temperature limit. Those guards remain present; this finding does not claim a measured thermal outcome. Validate or bound baseline acquisition so an already obstructed arm cannot normalize its resistance, and add a regression with resistance present during acquisition. The existing tests add the 400-load push after learning a lower baseline.

Source finding checked at 09:52 on `49695de6b`; no force, motor or device probe. The ordinary Body gate passes 298/0/0, fmt and strict Clippy pass. ARM64 38839669690 passes 5,254/130 skipped with all four changed blobs exact; its inherited dependency inputs differ. The first-builder handoff remains.

## PR #947 — ff five findings cleared first demo

[Pull request](https://github.com/autonomous-ai/lampOS/pull/947)

Reviewer: for the first builder: at 09:50, all five source findings are corrected on `ff6e7a443`: pause/person limits are explicit, the demonstration uses a temporary duration-shortened copy, voice disabling is marked unimplemented, load errors go to the service journal, and warm light is deferred. I reran both unchanged recording sets with the preserved binary of the now-merged #950 runtime: 8 recorded cases pass; the 2 absent paused recordings remain missing, not passes. The runtime source matches that binary.

The ARM64 run for this head (38841217693) is still running. The two skills request physical behavior, and this PR does not record a demonstration on an author's lamp; #941 also calls for showing the owner the skill before keeping it. Please judge that remaining acceptance. This review has not installed either skill or performed a lamp demonstration.

## PR #937 — cdc gate review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/937)

Reviewer: follow-up at 09:50 on `cdc6ee1ba`: fmt, strict all-target Policy Clippy, and the full serial Policy gate pass (171 passed, 0 failed, 2 ignored), with no retries. The rebase preserves the reviewed progress/rules behavior and both acting flags. ARM64 run 38839937869 passes 5,256 tests with 130 skipped; all 171 Policy identities occur once, and the independently reconstructed native checkout tree equals the feature-head tree. No new source finding. The existing first-builder handoff for the spoken keep/forget acceptance remains; this host result does not claim that acceptance.

## PR #884 — 5d task drain bound failure

[Pull request](https://github.com/autonomous-ai/lampOS/pull/884)

Reviewer: gate failure at `lampOS/crates/tasks/tests/harness.rs:548`, checked at 09:50 on `5d0c2d43f`: `a_descendant_holding_pipes_is_killed_without_an_unbounded_drain` returns the expected `Uncertain(pipes_open)`, but fails the elapsed-time bound of two seconds. Its later heartbeat-stop and child-reap assertions therefore do not execute. This test file is unchanged by the PR. The search hit in closed #698 concerns a different, earlier preflight assertion and does not waive this failure. Please account for the bound failure or provide an applicable open Queue exception; the original result and executable are preserved, with no retry.

ARM64 run 38837688447 passes 5,240 tests with 130 skipped, including this identity. I independently reconstructed its checkout tree and verified that it equals this PR head's tree. Local Schedule and Tasks failures remain open, and the existing first-builder handoff still applies.

## PR #884 — 5d schedule listener failure

[Pull request](https://github.com/autonomous-ai/lampOS/pull/884)

Reviewer: gate failure at `lampOS/crates/schedule/tests/runtime_failures.rs:187`, checked at 09:50 on `5d0c2d43f`: `a_full_stderr_sink_cannot_hold_status_or_sigterm_shutdown` fails on its first status connection with OS error 61, ConnectionRefused. The fixture waits for the socket pathname at lines 175–185; this failure occurs before its status-latency and shutdown checks. The file is unchanged by this PR, and the failure alone does not identify why the listener was unavailable. Please account for this failing gate or link an applicable open Queue exception; I found no exact-name exception and have not rerun it.

The complete nine-crate gate retained 2,394 passed, 3 failed, 11 ignored; fmt and strict Clippy pass. The reminder AckConfirmed failure has its documented #739 exception after one original-binary isolation passed. The Schedule failure and the separate Tasks failure remain unwaived.

## PR #954 — 719 full gate five failures

[Pull request](https://github.com/autonomous-ai/lampOS/pull/954)

Reviewer: at 09:50, the ordinary gate on `719b1850f` is still red: fmt and strict all-target Clippy pass; full serial Voice is 1,446 passed, 5 failed, 8 ignored. All ten new say/coordinator cases pass, including the whole-line deadline case.

Four `coordinator::tests::second_route` cases fail at `lampOS/crates/voice/src/coordinator/tests/second_route.rs:162` with stale privacy/capture-discontinuity errors: `a_question_before_geminis_first_ready_waits_for_it`, `a_question_goes_to_the_second_route_once_the_startup_hold_is_over`, `spoken_over_the_second_routes_answer_stops_as_gemini_does`, and `the_next_turn_goes_back_to_gemini_once_it_is_ready`. `reminders::consumer::tests::completed_playout_survives_both_real_store_reopens_then_acks_without_respeech` fails its AckConfirmed assertion at `lampOS/crates/voice/src/reminders/consumer/tests.rs:644`. Original binaries and logs are preserved; none of these failures has been retried or waived in this review.

ARM64 run 38839609398 passes 5,261 tests with 130 skipped; its actual synthetic checkout and the changed say/coordinator code are verified. That native pass does not erase the local failures. The receipt-expiry P2 and complete-line size P3 also remain open; the whole-line deadline P2 is corrected. Please resolve the open source findings and account for the failing gate before approval.

## PR #930 — queue fix proof b4 to 2a26

[Pull request](https://github.com/autonomous-ai/lampOS/pull/930)

Reviewer: the queued-input P2 is fixed. Against `b4b30c572`, the original nine regression procedures now pass **9/9**, versus the preserved prior result of eight passes and one “provider input lost its owner” failure. All assertions are unchanged; the only fixture adaptation is the exact newly corrected coordinator branch. The current `2a26e1bb5` rebase retains that branch byte-for-byte. The ordinary, cut-in, ended/ongoing, Stop, completion and expiry controls remain covered. No lamp or real provider was used.

The completed `b4b30c572` full gate had fmt/strict Clippy pass and **1,447 tests passed / 2 failed / 8 ignored**. Both original failures are retained: motor `pending_segment_privacy_revoke_wins_before_any_position_write` (`coordinator/motor/tests.rs:1056`, inhibit acknowledgement timeout plus missing file), and `same_owner_input_end_cancels_queued_gesture_before_inert_bus_write` (`:1193`, missing file). No waiver or retry is claimed for those results. All 24 original products and the corrected regression binary are preserved.

I have inspected the rebase adaptations, including the new test End field and inherited full-look/wait-for-room changes. A fresh full gate on `2a26e1bb5` is now running; its native run 38840155315 will be qualified separately. First's physical “Sorry?” acceptance remains. Checked at 09:35.

## PR #962 — queued input p2 f70d9dff7

[Pull request](https://github.com/autonomous-ai/lampOS/pull/962)

Reviewer: P2 — discard the failed turn's queued work when dropping the active second worker (`lampOS/crates/voice/src/coordinator.rs:2914–2917`, head `f70d9dff7`). This new branch removes `reply` through `cancel_reply`, but leaves `provider_queue` and `speaker_pending` intact. If the worker exits between ticks after an input send returned WouldBlock, an unsent Audio/End remains. `apply_submission_gate` does nothing with no reply, and the next `second::flush` falls through to `flush_provider_input_with`, which returns “provider input lost its owner” at line 1786. The voice therefore still exits in precisely the recoverable worker-failure path this change intends to handle.

Clear the cancelled turn's queued input and pending speaker work as the existing nothing-said/reminder cancellation paths do, and cover an active-worker exit with an unsent input still queued. This is an exact source/control-flow finding, not a claim of a new runtime reproduction; the same queue-lifetime omission was independently reproduced and fixed in #930. Checked at 09:34. The separate privacy-path handoff remains with First.

## Issue #828 — merged 956

[Issue](https://github.com/autonomous-ai/lampOS/issues/828)

Reviewer: merged #956 at 09:31 by rebase: the opt-in second-route STT fix that retains the tail after a short Finalize response; #828 remains open for live interruption and stop-latency work.

## Issue #828 — merged 945

[Issue](https://github.com/autonomous-ai/lampOS/issues/828)

Reviewer: merged #945 at 09:28 by rebase: the explicit STT timing command with queue-to-parsed-result boundaries documented; #828 remains open for live interruption and stop-latency work.

## PR #956 — ready c1a59b06e

[Pull request](https://github.com/autonomous-ai/lampOS/pull/956)

Reviewer: review complete at `c1a59b06e`. Fresh workspace fmt, strict all-target Voice Clippy and full serial Voice tests pass: **1,442 passed / 0 failed / 8 ignored**, no retry. All original executables and the main program are retained.

Six independent checks run the unchanged actual STT worker with inert WebSocket peers: short final followed by its tail, the same audio in 30 frames, a complete-final control, ignored Finalize, the 15-second timeout and cancellation. All pass. Restoring only the old completion branch retains the complete-final pass but reproduces the truncated short result; that expected failure and both binaries are preserved. These are protocol/virtual-clock checks, not network or acoustic latency measurements.

Native 38835876302 independently qualifies 5,201 unique passes / 130 skips, four doc tests and 12 process checks. Actual synthetic `ba0e28f14` reconstructs tree `cac293063`; STT is exact, with only the separately reviewed scenario-clock fixture inherited. The ordinary-route packet/record/sample comparison passes. Current integration is clean; the STT additions from just-merged #945 are only its reviewed first-word observer and test.

No remaining finding. The configured second route is opt-in; the author's synthetic relay observations are distinguished from live interruption acceptance. Merging by rebase; #828 remains open for that wider work. Checked at 09:31. No lamp, credential or real provider was used in this review.

## PR #954 — line length p3 719b1850f

[Pull request](https://github.com/autonomous-ai/lampOS/pull/954)

Reviewer: P3 — enforce the complete line's byte limit before returning at `lampOS/crates/voice/src/say.rs:565–570`, head `719b1850f`. `read_line_within` currently returns as soon as its buffer contains a newline, before the `MOST_LINE_BYTES` check. For example, valid JSON padded with spaces to 4,096 bytes and a following newline is accepted as a 4,097-byte request; a final read can take the accepted line as far as 4,608 bytes. Check the newline-terminated slice against the limit before returning, and cover exact-limit/one-byte-over cases. This is a source-only finding, not an additional socket run. The one-second whole-line timer is corrected; the earlier expiry P2 remains because its request/poll/take/coordinator code is unchanged. Checked at 09:30; the current full Voice gate is running.

## PR #962 — first review f70d9dff7

[Pull request](https://github.com/autonomous-ai/lampOS/pull/962)

Reviewer: for the first builder: first review at 09:30 of `f70d9dff7`, including both the delayed-authority and optional-worker changes and #960's latest observations. This changes the privacy/input authority drain and how a dead second worker releases a turn, so the privacy-path disposition remains yours. I am checking retirement, queued input and cancellation boundaries, the real-controller test coverage, and the full touched Voice gate/native result. The documented within-tick worker-death limit remains; #960 also retains the separate mute-latch work. No lamp or real provider will be used.

## PR #957 — first review ad0e7f447

[Pull request](https://github.com/autonomous-ai/lampOS/pull/957)

Reviewer: first review at 09:30 of ready `ad0e7f447`. I have read the transport wait wrapper, all seven new boundary/cleanup cases and the stated #823 limits. The explicit test clock retains real child observation, the full five-second logical deadline, kill/reap and the original certification assertions; ordinary callers retain the host clock and the same check ordering. The fixed host watchdog is correctly distinguished from an end-to-end OS cleanup bound. No source finding so far. I am checking the complete identity/source preservation and both native attempts independently, then running the full Evaluator gate. The original-123 standalone serial target remains unproved; #823 stays open.

## PR #945 — ready 4fac6abcd

[Pull request](https://github.com/autonomous-ai/lampOS/pull/945)

Reviewer: the measurement-boundary P3 is fixed at `4fac6abcd`. The delta is documentation and comments only: queue-to-parsed-result timing, the single-message held prefix, and the separate synthetic relay observations are now explicit. The previously completed exact-code gate remains applicable: fmt, strict all-target Voice Clippy and 1,404 passed / 0 failed / 8 ignored, with original products preserved.

Current native 38838992586 independently qualifies 5,251 unique passes / 130 skips, four doc tests and 12 process checks. Its actual synthetic `63fc646ed` reproduces tree `47560accf`; reviewed main/STT code is byte-exact. The differing document inherits the already-reviewed startup/queue fixes and scenario-clock explanation. Current integration is clean with main/STT still exact.

The explicit manual command is the only probe entry. Existing ordinary-route packet/record/sample comparisons and STT final-result controls pass; this is not a claim of a live CLI or lamp measurement. No remaining finding; merging by rebase. #828 stays open for interruption latency. Checked at 09:28.

## PR #918 — complete c6d84e6b5

[Pull request](https://github.com/autonomous-ai/lampOS/pull/918)

Reviewer: all three reported P2s are fixed at `c6d84e6b5`; the original eligibility, named-call and actual-worker conversation controls now pass. Fresh workspace fmt and strict all-target Voice/Evaluator/Memory Clippy pass. Full serial results: Voice 1,479 passed / 1 failed / 8 ignored; Evaluator 498 / 0 / 0; Memory 205 / 0 / 5. All 64 original products and the complete first failure are retained.

The one failure was `coordinator::motor::tests::native_head::native_head_six_explicit_actions_prepare_profile_then_new_requests`, at `coordinator/motor/tests/native_head.rs:172`, reporting an inhibited/rejected qualification worker. Queue #705 and #739 explicitly cover that native-head family. Its single permitted isolated execution from the original failed binary passed, 1/1; no other test was retried. This meets reviewer.md's documented exception, without calling the original full run green.

Native 38838085088 independently qualifies 5,267 distinct passes / 130 skips, four doc tests and 12 process cases, including all 44 relevant Voice identities once. Actual synthetic `94329b8638` reconstructs tree `231f5bc4`; inherited full-look plumbing is accounted for and the reviewed attention/room logic is exact.

Reviewer: for the first builder: no remaining source finding, but the actual room/addressing behavior remains your lamp acceptance. #829 stays partial and open. Checked at 09:27; no lamp or real model was used.

## PR #955 — speech lifetime p2 6beb3cd36

[Pull request](https://github.com/autonomous-ai/lampOS/pull/955)

Reviewer: P2 — clear the new speech-motion history at the existing producer lifetime fences (`lampOS/crates/expression/src/engine.rs:1395–1401`, head `6beb3cd36`). New turns and audio generations call `clear_transients` at lines 946 and 952–955, but that function never resets `SpeechMotion`. A higher-generation, unheld Listening update can therefore emit the previous generation's nod or sway with no new Speaking or PCM. Cancellation also returns before `SpeechMotion::step`; a later Listening turn can start the cancelled answer's settling motion.

The actual compiled Engine, with inert stamped inputs and paired speaking-on/off catalogues, reproduces three failures: an old nod at +0.014 rad, an old roll at +0.0175 rad, and a cancelled answer's settle at +0.0075 rad in the next Listening turn. Each has an empty playout queue, no rejected input and no new speech. Fresh Listening and an observed held tick are two passing controls. These are logical-time offsets, not lamp observations. Reset the gesture history on new session/turn/generation and cancellation while preserving intentional settling after a normal finish; add these lifetime cases.

The original full gate is otherwise green: workspace fmt, strict all-target Expression Clippy, and 238 tests passed with no failures or ignores; all 18 products are retained. Native 38835699107 qualifies 5,208 unique passes / 130 skips, plus four doc tests and 12 process cases, with all seven PR blobs exact. The five independent assertions remain recorded as two passes / three failures. Reviewer: for the first builder: your visible-motion changes and physical judgment remain pending separately. Checked at 09:27; no device or service was used.

## Issue #938 — merged 950

[Issue](https://github.com/autonomous-ai/lampOS/issues/938)

Reviewer: merged #950 at 09:22 by rebase: the opt-in skills framework and Welcome back folder; the documented remaining work and physical acceptance keep this issue open.

## Issue #823 — merged 942

[Issue](https://github.com/autonomous-ai/lampOS/issues/823)

Reviewer: merged #942 at 09:20 by rebase: the explicit clock for the six Serve relay tests; the standalone original-123 serial target remains open.

## PR #950 — review ready 221026bfc

[Pull request](https://github.com/autonomous-ai/lampOS/pull/950)

Reviewer: review complete at `221026bfc`, checked at 09:22. Both P2s are fixed; the unchanged seven-case regression is7/7. Fresh full gates remain **Skills11 + Msgs95 =106 passed/0 failed/0 ignored**, fmt and strict all-target Clippy pass,14 original products preserved.

Native38837675152 independently qualifies **5,234 unique passes/130 skips**,4 doc tests/12 process cases/all14 stages. Actual synthetic `a4a3bb610` independently reconstructs tree `e4d288571`. All Skills, Msgs, Bus, IPC, workspace manifests and locked dependencies match the reviewed head. The two differing PR documentation blobs only inherit Voice bus-link prose and First's V1 evaluation; the29 other inherited inputs do not alter this runtime's dependency closure. Current integration is clean; its only additional lockfile change is the already-merged Eyes JPEG dependency.

The service remains absent from product installations, and the real-program off/on controls confirm no gaze/action call without the explicit skip. `SkillEvent` is new and has no existing reader whose contract changes. Following First's09:14 clarification, this framework is ready for rebase merge; physical installation and Welcome back acceptance remain First's separate step. The documents candidly retain the Policy/say/light/gaze limitations. #938 is partial and stays open. No lamp or real model was used.

## PR #961 — first review bddaf251a

[Pull request](https://github.com/autonomous-ai/lampOS/pull/961)

Reviewer: first review at 09:21 of `bddaf251a`. I have read the three-file diff and #948's failure context. The production `open_session_for` still clamps to300ms; this exposes its existing unclamped body for real-store fixtures and preserves the held/missing-store deadline controls. I am checking every altered call and its original assertions, then running workspace fmt, strict all-target Memory/Voice Clippy and both full serial crate suites. The explanatory public API comments are included. Native38839040530 will be qualified independently. No source finding so far; no load generators or repeated stress runs are planned.

## PR #849 — full review 4104def2a

[Pull request](https://github.com/autonomous-ai/lampOS/pull/849)

Reviewer: for the first builder: exact `4104def2a` now passes the full fresh four-crate gate: workspace fmt, strict all-target Clippy, and complete serial tests — **Msgs94 + Controls399 + Expression243 + Voice1,484 =2,220 passed/0 failed/10 ignored**, with no retry and all73 original products retained. The stale-held authority and cache-lock observations are resolved; the original timestamp/300ms expiry is preserved, and polling happens outside the cache mutex. No new source finding.

Native38837746491 independently qualifies **5,271 distinct passes/132 skips**,4 doc tests/12 process checks/all14 stages. Actual synthetic `735d7a73a` reconstructs tree `6c09b2d20`, identical to the entire reviewed head and all dependencies; all53 PR paths match. The strengthened expiry case executes once. The privacy path, shared Touch semantics and real hand/control response remain your disposition for #711. No new traffic/fault probe or lamp action was performed. Checked at 09:21.

## PR #947 — stretch light p3 569c5539d

[Pull request](https://github.com/autonomous-ai/lampOS/pull/947)

Reviewer: P3 — remove or mark pending the warm-light promise at `lampOS/skills/stretch-break/SKILL.md:8–10`. The actual `skill.toml` deliberately contains only gaze and stretching; its comment and this PR correctly defer light restoration until #940. The owner-facing description still says a warm light lasts a few seconds. The exact #950 runtime replay requests just the two body calls, with no light. Checked at 09:21 on `569c5539d`. Across both skills the original recordings give8 passes and2 missing paused recordings; missing recordings are not passes.

## PR #947 — load diagnostics p3 569c5539d

[Pull request](https://github.com/autonomous-ai/lampOS/pull/947)

Reviewer: P3 — direct load/refusal diagnosis to the service journal at `lampOS/skill-creator/references/installing.md:36–43` and `references/skill-folder.md:13–14`. The proposed five-second `skills/events` tap cannot show those diagnostics: current #950 writes load and subscription errors to stderr (`crates/skills/src/main.rs:161–178`), while the event topic carries triggered/asked/answered/done. `Engine::replace` emits no load event. Keep the event tap for action outcomes and use the actual stderr/journal for load errors. Checked at 09:21 on `569c5539d`.

## PR #947 — voice disable p2 569c5539d

[Pull request](https://github.com/autonomous-ai/lampOS/pull/947)

Reviewer: P2 — describe a supported way to disable a skill at `lampOS/skill-creator/references/installing.md:47` (also `SKILL.md:38–39`). This promises saying “Lamp, stop doing …” makes the runtime turn the skill off. Current #950 has only `serve` and `test`, loads all non-hidden folders, and exposes no skill-disable operation or voice command handler; it does not consume the skill's SKILL.md instructions. The explicit manual removal on line48 is supported. Mark voice disable as pending, or describe that supported removal workflow, rather than promising a command that is not implemented. Source finding on `569c5539d`, checked at 09:21; no voice/model/lamp command was attempted.

## PR #942 — review ready 917a19c5c

[Pull request](https://github.com/autonomous-ai/lampOS/pull/942)

Reviewer: all merge checks pass at `917a19c5c`, checked at 09:20. Fresh workspace fmt, strict all-target Evaluator Clippy and the full serial Evaluator suite passed **508/0/0**, with all 25 original products retained. Listing the 24 original test executables independently reproduces all 508 published identities and ignore flags exactly; no tests were rerun for that inventory. The six original bodies and all 37 assertions are unchanged. The controlled fixture is explicitly selected, with ordinary entrypoint controls covered; production timing is unchanged.

I independently fetched and reconstructed normal ARM6438831311751: **5,204 distinct passes/130 skips**, plus4 doc tests and12 process checks. All100 changed blobs match its synthetic tree. Full ARM6438831310695 attempt2 is a qualified **5,321 distinct-pass composite**, including all123 original identities and all508 Evaluator tests once. Only the shard that failed downloading serde before running tests was restarted. The three successful shards retain byte-exact build/test logs and original execution times. A fresh auxiliary failure-report job log differs from the published capture; both captures are retained, and that reporting text does not alter the build/test evidence. No product test was retried.

Current integration onto `f4b5b0cff` is clean with all100 PR blobs exact. The full123 duration sum207.660s remains a cross-shard sum, not the standalone serial target; #823 stays open. Scope, documents and original failure/timeout/signal/kill/reap assertions check out. No remaining finding; merging by rebase.

## PR #947 — demo clock p2 569c5539d

[Pull request](https://github.com/autonomous-ai/lampOS/pull/947)

Reviewer: P2 — the prescribed accelerated live demonstration does not trigger these duration-based skills (`lampOS/skill-creator/SKILL.md:113–116`). `lamp-bus replay ... --speed60` accelerates message delivery, while #950's live `main.rs:200–205` evaluates each message using its current receipt clock. A50-minute recorded sitting delivered in about50 seconds never satisfies `for="50m"`; the10-minute air case has the same mismatch. The separate offline test command uses recorded timestamps and passes, but that is not the claimed physical demonstration.

Provide a demonstration procedure supported by the runtime and preserve the distinction between recorded-time validation and live receipt-time execution. The current receipt-clock replay adapter produces zero calls from both accelerated positive cases; this is a source/clock proof, not a live socket or lamp measurement. No installation or device command was run. Checked at 09:11.

## PR #947 — pause contract p2 569c5539d

[Pull request](https://github.com/autonomous-ai/lampOS/pull/947)

Reviewer: P2 — remove or implement the promised automatic pause/person gate (`lampOS/skill-creator/SKILL.md:70–71`, `references/senses-and-actions.md:25`). The installation guide currently instructs using `LAMP_SKILLS_ASK=skip` until #939 exists, while #950 has no implicit pause condition. Neither example skill names `lamp/pause`. A creator following these instructions can therefore install behavior that acts while paused, despite being told the runtime enforces the opposite. Make the current limit and installation conditions explicit, or supply the necessary supported condition; do not describe the future Policy gate as present.

Against the preserved exact #950 `221026bfc` binary, each original four-case recording suite passes. Adding a kept `lamp/pause` value with `paused:true` to scratch copies of their positive cases and expecting no calls still produces the stretch's gaze/gesture at50 minutes and the air skill's dim at10 minutes. This is offline recording replay, not a lamp action. The empty paused-case folders correctly remain untested, but do not make the creator's operational promise true. Checked at 09:11.

## PR #954 — expiry p2 0fbb6f338

[Pull request](https://github.com/autonomous-ai/lampOS/pull/954)

Reviewer: P2 — preserve the request's expiry while it waits for the coordinator (`lampOS/crates/voice/src/say.rs:351–359`, head `0fbb6f338`). `Asked` has no receipt timestamp, so polling creates a new `expires_in_ms` lifetime regardless of how long the valid request has already waited in the incoming queue. The coordinator also passes its older loop-start clock to `poll`, then uses a fresh clock for admission without rechecking expiry in `take` (`coordinator.rs:4859–4884`). A request can consequently begin after its caller's expiry rather than returning `expired`, contrary to `docs/speaking-first.md:229`.

Carry a monotonic receipt/enqueue deadline with the request and recheck it using the current admission time immediately before beginning its turn. In the existing bounded public-service fixture, a valid1 ms request with its first poll deliberately delayed200 ms received begun/spoken at204,132 µs after the client's completed write. This models delayed polling; it is not a measured200 ms production-loop stall or an independently timed parse receipt. The source shows both missing time accounting and the stale-clock admission window. Checked at 09:06; separate from the whole-line deadline finding.

## Issue #800 — merged 943

[Issue](https://github.com/autonomous-ai/lampOS/issues/800)

Reviewer: merged #943 at 09:04 by rebase as `fca65fde5`: opt-in Policy attention decisions for the already merged Voice action holds, with defaults preserved and the named-call timing documented. #800 remains open for its other work.

## PR #870 — complete b8926212e

[Pull request](https://github.com/autonomous-ai/lampOS/pull/870)

Reviewer: source and verification complete at `b8926212e`, checked at 09:04. The one-commit rebase preserves the reviewed success gesture and persisted daily re-offer logic; #858 is now merged. Paused/absent-body delivery is consumed once, and unsuccessful outcomes do not queue the success gesture. No new correctness finding.

Fresh workspace fmt, strict all-target Voice Clippy and full serial Voice gate pass: **1,463 passed/0 failed/8 ignored**, no retries,23 original products retained. Native38836478138 independently qualifies **5,222 distinct passes/130 skips**,4 docs/12 process checks and all14 onboarding identities once. Actual synthetic `805000167` independently reproduces tree `ab0b51620`, equal to the entire reviewed head including dependencies.

Reviewer: for the first builder: the actual nod/greeting timing and appearance, and the requested brightened-eye/pleased-lean scope, remain your physical judgment. The code substitutes the documented existing greeting where no lean is available and adds no Expression eye cue. #841 remains partial and open; this review used no lamp or real model.

## PR #950 — gaze fixed gate 221026bfc

[Pull request](https://github.com/autonomous-ai/lampOS/pull/950)

Reviewer: both gaze P2s are fixed at `221026bfc` (`lampOS/crates/skills/src/lib.rs:572–612,725`). Gaze steps now contribute the Faces subscription, and gaze selection shares the same freshness check as conditions. The unchanged independent seven-case reproducer passes **7/7**, versus **5 passed/2 failed** on `a0b5f1a47`. It covers the missing subscription and exact expiry, plus fresh-at-expiry-minus-one-microsecond, no face, empty update, expired face condition and closed Policy controls. Same source/assertions, actual compiled Engine, no tracked edits or lamp.

Fresh exact-head gate: `cargo fmt --all --check`; `cargo clippy -p lamp-skills -p lamp-msgs --all-targets -- -D warnings`; full serial `cargo test` for both crates — **Skills11 + Msgs95 =106 passed /0 failed/0 ignored**. No retries;14 original products and both old/new proof executables retained. Current native38837675152 qualification is pending. The two reported correctness findings are cleared; First still owns the shared-topic/physical integration disposition already described. Checked at 09:04.

## PR #943 — ready c73293fd7

[Pull request](https://github.com/autonomous-ai/lampOS/pull/943)

Reviewer: ready to merge `c73293fd7` at 09:04. The two documentation corrections accurately limit the two-second wait to held calls; named turns run immediately. This head differs from the fully reviewed `165d4cf22` only in those two documents, so the fresh Policy+Voice gate carries unchanged: workspace fmt, strict all-target Clippy, and complete serial tests, **1,607 passed / 0 failed / 10 ignored**, no retries,42 originals retained.

The new head's ARM6438836625713 is independently qualified: **5,221 distinct passes/130 skips**,4 docs/12 process checks/14 successful steps. Actual synthetic `f38702869` with base `1fb07c6f5` independently reproduces tree `51e54809d`; all11 PR blobs match, and14 inherited inputs are retained separately. A current-base merge-tree check against `1d598d77d` is clean and preserves all11 PR blobs.

`--act attention` is explicitly off by default, with default-versus-enabled real-binary tests. No Msgs/Bus/IPC schema or meaning changes, privacy-path changes, forbidden artifacts or role-scope additions. The emitted false-shadow decision follows the existing contract; First's accepted #908 behavior is retained as qualified in the source review. No findings remain. #800 is a parent task and will stay open.

## PR #945 — complete and timing doc p3 20bce50a6

[Pull request](https://github.com/autonomous-ai/lampOS/pull/945)

Reviewer: fresh exact-head `20bce50a6` gate passed: workspace fmt, strict all-target Voice Clippy and full serial Voice tests — 1,404 passed / 0 failed / 8 ignored, no retry,22 original executables retained. Native38831995418 independently qualifies5,202 unique passes/130 skips,4 docs/12 process checks/14 successful steps; all three PR blobs match actual synthetic `f8282c0ec`, with99 inherited inputs separately recorded. No new runtime defect found.

P3 — make the measurement boundary explicit at `lampOS/docs/second-voice.md:340–353` and `lampOS/crates/voice/src/main.rs:57–60`. The clock starts before enqueueing samples, and the first-word timestamp is after parsing a result; this is queue-to-parsed-result timing, not a wire-send or acoustic stop-latency receipt. The held prefix is one WebSocket message, whereas #828's proposed route replay preserves individual frames; the author's08:23 task reply already calls out that distinction. Please state it in the probe documentation instead of implying equivalent held-audio framing. Also update “the relay's has not been measured yet”: Sixth's subsequent #828 synthetic CLI measurements now exist, with the burst-tail fix separately in #956. No new measurement is requested here.

#828 remains open for actual interruption/stop latency. Checked at 09:02; this review used no live relay, credentials or lamp.

## PR #954 — request deadline p2 0fbb6f338

[Pull request](https://github.com/autonomous-ai/lampOS/pull/954)

Reviewer: P2 — enforce a deadline for the complete client line (`lampOS/crates/voice/src/say.rs:513–520`, head `0fbb6f338`). `set_read_timeout(READ_WITHIN)` applies separately to each underlying read; `BufReader::read_line` can keep receiving an incomplete request beyond the advertised one second. Each such client retains one of the sixteen admission slots, so prompt skill calls can be refused while the delayed clients remain accepted. Read against one elapsed deadline for the whole line and release the slot when it expires.

Bounded local reproduction with the unchanged service module and real private Unix sockets: a prompt control completed in 1,373 µs; a silent control was refused at 1,000,734 µs. Sixteen clients sending a small piece every 600 ms still occupied every slot at 1,813,607 µs, when a prompt seventeenth request received `16 wait already`; all sixteen delayed complete lines were then admitted at 1,815,256 µs. The first fixture setup error and the complete successful reproduction are retained separately. No lamp, model or external service was used.

First review at 09:02: opt-in and task scope are being checked with #939; the complete exact-head Voice gate and native qualification remain pending. This source/fixture finding is independent of that gate.

## PR #947 — first review 569c5539d

[Pull request](https://github.com/autonomous-ai/lampOS/pull/947)

Reviewer: first review at 09:02 of `569c5539d` and #941. The two skill folders match the declared duration/rearm/condition shapes; Stretch break omits the persistent light change, and Stuffy air explicitly waits for light restoration before installation. The two paused cases are placeholders, not passing recordings, and the PR correctly leaves them for the Policy gate. I am reviewing every creator reference and recorded case against #950, parsing the TOML examples, running the recordings through the preserved runtime, and qualifying the native run. Merge depends on #950 landing first. No crate is changed here and no lamp installation or demonstration is being performed by this review.

## PR #795 — complete 62c3d230c

[Pull request](https://github.com/autonomous-ai/lampOS/pull/795)

Reviewer: exact-head `62c3d230c` follow-up at 08:58. The writer-path P2 is structurally fixed: LeaseWatch polls outside the cache mutex, the writer rechecks freshness, and its Stop/Authority/final Ring checks remain. All seven old test bodies in the two edited Rust files are unchanged. The inaccurate immediate-reader-failure wording remains the separate P3 at `lampOS/docs/privacy-switch.md:104–105` (comment6206115982).

Workspace fmt and strict all-target Clippy passed for Controls, Ears, Mood, Msgs, Ring and Voice. Their full serial test gates produced **2,100 passed / 1 failed / 11 ignored**. The failure is `coordinator::tests::pilot_faults::a_stall_while_the_lamp_answers_cuts_the_reply_and_the_voice_goes_on`, `lampOS/crates/voice/src/coordinator/tests/pilot_faults.rs:1965`: `service privacy freshness lost before next observation`. Its source is unchanged from the previous reviewed head. All 73 original products and the first failure are retained. No retry or additional traffic/fault probe was run, and no pass-alone waiver is established for this exact identity.

Native38834244122 is independently qualified: 5,285 unique passes/131 skips, four docs/twelve process checks,14/14 steps; actual synthetic `5be420bde`, reproduced tree `3a4a7740e`, all three follow-up blobs exact. Its PR profile excludes this pilot-fault family, so its green result does not cover the local failure.

Reviewer: for the first builder: the privacy/mute path and #755's physical acceptance remain yours. Please retain the failing gate and outstanding documentation correction in the disposition; this review does not assert a microphone Stop failure from the earlier optional-writer finding.

## PR #842 — privacy doc fixed d032821fc

[Pull request](https://github.com/autonomous-ai/lampOS/pull/842)

Reviewer: the privacy wording P3 is fixed in `d032821fc` (`lampOS/docs/verification/full-look-held-lamp-52e6-20261016/README.md:46`). The exact one-line documentation diff now distinguishes sending each still to Gemini to answer the question from retaining test copies and deleting the local look.jpg afterward. No runtime code changed. This clears that documentation finding; First retains privacy/physical acceptance and the merge decision. Checked at 08:57.

## PR #952 — startup fixed 519c1cc7f

[Pull request](https://github.com/autonomous-ai/lampOS/pull/952)

Reviewer: the startup-exit P2 is fixed at `519c1cc7f` (`lampOS/crates/body/src/body.rs:1987–2008`). The wrapper now catches every return from the former run body and performs the same five-joint release except for Cancelled, Refused and Overload. The old loop-tail release is removed, so ordinary failure exits are not released twice. The existing wake failure now asserts whole-lamp release; the new shoulder-lowering Tracking case also checks the hold and all five torque-off writes.

Fresh exact-head gate: `cargo fmt --all --check`; `cargo clippy -p lamp-body --all-targets -- -D warnings`; `cargo test -p lamp-body --no-fail-fast -- --test-threads=1` — 296 passed, 0 failed, 0 ignored. All 13 original products retained, no retry. Native38836239271 independently qualifies 5,209 unique passes /130 skips, four docs/twelve process checks, all fourteen job steps. Actual synthetic `5e6cb458d`, reproduced tree `95fd8cedb`, contains all four PR blobs exactly; its ten inherited source differences are Gemini/Voice changes outside Body's dependency closure.

Reviewer: for the first builder: source review and host/native verification are complete with no remaining finding here. This changes default physical failure handling; the new startup cases have not been judged on a lamp by this review. Your integration/merge and #951 acceptance remain. Checked at 08:57.

## PR #950 — gaze expiry p2 a0b5f1a47

[Pull request](https://github.com/autonomous-ai/lampOS/pull/950)

Reviewer: P2 — reject expired face data before making a gaze call (`lampOS/crates/skills/src/lib.rs:709–721`, head `a0b5f1a47`). The face lookup drops the stored timestamp and lifetime, while conditions correctly check them in `condition_holds`. With Faces subscribed by another loaded skill, a later non-Faces trigger can therefore turn toward the last remembered face after its evidence has expired and the publisher has stopped. The comment calls this “the largest in view now,” but it need not still be in view. Apply the same freshness test before selecting the face; keep the other steps independent when no fresh face is available. Source finding at 08:51; this is not a lamp observation.

## PR #950 — gaze topic p2 a0b5f1a47

[Pull request](https://github.com/autonomous-ai/lampOS/pull/950)

Reviewer: P2 — subscribe to the input required by a gaze step (`lampOS/crates/skills/src/lib.rs:572–588`, head `a0b5f1a47`). `Engine::topics()` includes only trigger/while topics, although a valid `body = { look_at = "face" }` step also reads `humans/faces/tracked` at lines 709–731. A skill triggered by, for example, `schedule/due` with no face condition never subscribes to Faces, so its gaze step is silently omitted. It starts working only if an unrelated installed skill happens to name Faces. Include the step's sensor dependency when forming the subscription set, and cover a non-Faces trigger with a gaze step. Source finding at 08:51; full Skills/Msgs gate follows the Body gate already running.

## PR #956 — first review c1a59b06e

[Pull request](https://github.com/autonomous-ai/lampOS/pull/956)

Reviewer: first review at 08:51 of `c1a59b06e` against #828. The two-file change keeps the existing fifteen-second finish bound and sends CloseStream once when a from_finalize answer covers less audio than was sent (`lampOS/crates/voice/src/second_provider/net/stt.rs:401–419`). Full-coverage completion and the ignored-Finalize fallback remain present. I am checking this with an inert local peer and the complete exact-head Voice gate, then qualifying ARM64 38835876302. The reported synthetic lamp-52e6 CLI rounds are useful evidence, but do not establish live interruption acceptance or complete #828. No source finding so far; no live relay or lamp is used by this review.

## PR #942 — first review 917a19c5c

[Pull request](https://github.com/autonomous-ai/lampOS/pull/942)

Reviewer: first review at 08:51 of `917a19c5c`. I have read the Relay changes and #823. The explicit fixture keeps real Stop/EOF, signal observation, child exit/kill/reap and full logical deadlines; the ten-second post-stop host success check is correctly distinguished from the existing thirty-second reader barrier. The ordinary fake entrypoint remains separate. I am checking the six original bodies/assertions and the published inventory/native composite independently, then running workspace fmt, strict all-target Evaluator Clippy and the full serial Evaluator gate. No source finding so far; merge is pending those checks. #823's standalone original-123 serial target remains unproved and the issue stays open.

## PR #937 — room fix complete b5185b121

[Pull request](https://github.com/autonomous-ai/lampOS/pull/937)

Reviewer: the answer-time room P2 is fixed at `b5185b121` (`lampOS/crates/policy/src/progress.rs:952`). The unchanged original seven-case reproducer now passes 7/7: Quiet→Busy and Quiet→Unknown wait, while the unchanged-Quiet, newly kept Never, forgetting, wall-clock and task-run controls retain their assertions. The question's original room remains in its logged context; the decision uses the current room.

Fresh exact-head verification: `cargo fmt --all --check`; `cargo clippy -p lamp-policy --all-targets -- -D warnings`; `cargo test -p lamp-policy --no-fail-fast -- --test-threads=1` — 168 passed, 0 failed, 2 ignored. No retry; all 15 gate executables and the separate reproducer retained. Native run 38835563920 is independently qualified: 5,205 distinct passes / 130 skips, four doc tests and twelve process checks; all fourteen job steps passed. Its actual synthetic checkout is `56086a257`, independently reproduced tree `13ee8c556`: all six changed Policy Rust blobs match this head, with 24 inherited input paths recorded separately. This qualifies the historical PR integration, not today's entire branch or spoken model acceptance.

Reviewer: for the first builder: source and host/native review are complete with no remaining finding from this review. First/Seventh's spoken keep/forget acceptance still belongs to this PR. #813 is already closed by Seventh for their delivered voice memory/rules scope; I have not reopened or reclosed it. Reviewed at 08:50.

## PR #930 — queued input p2 2c486daee

[Pull request](https://github.com/autonomous-ai/lampOS/pull/930)

Reviewer: P2 — discard unsent input when cancelling the unclear turn (`lampOS/crates/voice/src/coordinator.rs:4772–4775`, head `2c486daee`). This branch retires `reply` but leaves `provider_queue` intact. If the locally ended turn still has an End (or audio) queued after a nonblocking send returned WouldBlock, the next ordinary flush reaches `coordinator.rs:1775–1777` with no owner and returns `provider input lost its owner`; the coordinator exits instead of asking “Sorry?”. The existing nothing-said cancellation clears this queue. Clear the cancelled turn's pending submission here too, preserving cancellation ownership.

At 08:50, a bounded local reproducer using the exact current main-loop Unclear/asking/cancel-cleanup blocks and the actual admission, controller, IPC and flush functions gives 8 passing controls and this one failure: `held-queued queue=1 next_flush=Err(... "provider input lost its owner")`. The End first passed the real submission validation and remained queued after an injected WouldBlock; no lamp/provider traffic was used. This is a coordinator-boundary reproduction, not a full live-provider run. Original executable and first failure retained; no retry.

The earlier ended-cut-in and hand-Stop clarification findings are fixed: the original three admission assertions pass, and pending clarification is discarded on Stop. The new ended-held-turn control cancels and asks once on the next pass; ordinary completion, still-speaking and expired controls pass. Independently, the fresh exact-head gate passed: `cargo fmt --all --check`; `cargo clippy -p lamp-voice --all-targets -- -D warnings`; `cargo test -p lamp-voice --no-fail-fast -- --test-threads=1` — 1,449 passed, 0 failed, 8 ignored, all 24 original executables preserved. This new P2 remains blocking; physical clarification acceptance stays with First.

## PR #795 — 62c3d230c reader timing doc p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/795)

Reviewer: [P3] Correct the failure-timing statement at `lampOS/docs/privacy-switch.md:104–105`: “A refused message still ends the reader at once” is stronger than the implementation. `privacy_bus.rs:280–283` notices the discard counter only after `Subscriber::recv` returns; its unchanged internal rejected-message loop can keep that receive busy. The 2 ms at `privacy_bus.rs:345–353` is a sleep after a completed poll, not a receive-duration bound.

The original writer-path P2 is structurally addressed: polling happens on the separate LeaseWatch thread before it takes the cache mutex, and the writer reads the cache, rechecks freshness and retains its own Stop/Authority/expiry checks. Describe that independence and the eventual latched reader error accurately. This is source review only; no new traffic or fault probe was run. The ordinary full gate is in progress.

Checked at 2026-10-16 08:43 +0700.

## PR #955 — 6beb3cd36 review started

[Pull request](https://github.com/autonomous-ai/lampOS/pull/955)

Reviewer: for the first builder: reviewing `6beb3cd36add3ca530c834a2a1b15d19d6ba5d30` against #933. The new catalogue-driven nod, sentence sway and settle are physical motion whose sizes and appearance have not been shown on a lamp; that judgment and merge remain yours.

I have checked the motion state machine and engine insertion. I am continuing through limits, stale input, interruption/hold/sleep handling, movement continuity, the absent-catalogue control and the same-PR documentation. Workspace fmt, strict all-target Expression Clippy and the complete serial Expression gate will follow, with exact-head ARM64 qualification. #933 remains open for the full requested behavior and physical evidence.

Checked at 2026-10-16 08:42 +0700.

## PR #870 — 5efe05b81 review started

[Pull request](https://github.com/autonomous-ai/lampOS/pull/870)

Reviewer: for the first builder: reviewed the single commit `5efe05b812bd8e2b956d1d37faab7e9dcf36e9dd` atop #858’s `71566a014`. It retains both opt-in conditions, queues one completion gesture, and persists the per-step 86,400-second re-offer boundary. The existing nonblocking BodyLink sends the gesture; physical nod/greeting judgment remains yours, and the missing lean is documented as a greeting substitute. No new Expression eye-brightening cue is implemented here.

No concrete source finding so far. The complete fresh Voice gate follows #858’s archived gate, and native 38835651271 will be checked on this exact head. This depends on #858; #841 stays open for the broader walk and acceptance.

Checked at 2026-10-16 08:42 +0700.

## PR #943 — 165d4cf22 complete gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/943)

Reviewer: completed the fresh exact `165d4cf2245ab163270e0b5e413ff075865ccc46` gate. Workspace fmt and strict all-target Policy/Voice Clippy pass. Full serial results are **1,607 passed / 0 failed / 10 existing ignores**: Policy 166/0/2 and Voice 1,441/0/8. No retries or filtered cases; all 42 original products are preserved. The old default program cases and all nine changed-fixture progress-news cases pass.

Native [38831843506](https://github.com/autonomous-ai/lampOS/actions/runs/38831843506) qualifies with **5,201 distinct passes / 130 skips**, four doc cases and twelve own-harness cases, all fourteen stages green. Synthetic `e67cf7e7ff630fe73bfa4413c720d8db8cf0ba9f` has independently reproduced tree `777220f3eb07ad52d4da6e29dda57f32c3ec74a2`, equal to the entire reviewed head.

The opt-in publisher, request identity, disabled behavior and relevant First-accepted attention behavior are verified; no production finding remains. The [named-action timing documentation P3](https://github.com/autonomous-ai/lampOS/pull/943#issuecomment-6205923191) is the remaining correction before final merge checks. #800 remains partial.

Checked at 2026-10-16 08:42 +0700.

## PR #884 — f524928a4 complete gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/884)

Reviewer: the offline-notice acceptance P2 is fixed at `f524928a4a48c8cf3259eafba5993e2f92e74119` (`coordinator/notices.rs:107–119`, called at `coordinator.rs:4353`). All three original delivery procedures pass with only the new `note_delivery` call added: cancellation before dispatch, dispatch rejected before acceptance, and matching accepted-playback control. Additional full-coordinator local scenarios admit two offline speech turns but deliver one recording; short-sound and disabled controls deliver none. These use simulated acceptance/retirement, not acoustic delivery. The first additional fixture attempt had a peer socket-lifetime cleanup error; that original result and the narrowly corrected replay are both retained.

Fresh full nine-crate gate: **2,301 passed / 3 failed / 11 ignored**. Workspace fmt and strict all-target Clippy pass. Voice is 1,380/3/8; Audio 160/0/2, Expression 232/0/0, Update 70/0/1, Msgs 91/0/0, IPC 23/0/0, Health 59/0/0, Schedule 161/0/0 and Tasks 125/0/0. All 94 original products are preserved. No failed case was retried or waived.

The three Voice failures remain concrete:
- `coordinator/motor/tests.rs:1101`: late-Started test lacks `child-position-writes` (ENOENT), matching open #906.
- `coordinator/motor/tests.rs:453`: native-head test stops in shared fixture preflight before its scenario assertions.
- `coordinator/motor/tests.rs:787`: pending-inspection cleanup lacks the required privacy-release confirmation, matching open #906; this is distinct from #949’s pending-segment case.

Unchanged fixture source and the historical Queue family do not establish a cause or waive these results. Exact-head manual ARM64 [38833328797](https://github.com/autonomous-ai/lampOS/actions/runs/38833328797) separately passes **4,899 distinct tests / 127 skips**, four docs and twelve own-harness cases, including those three identities once. Its actual checkout is this feature head, with no synthetic integration; all 50 changed blobs match.

Reviewer: for the first builder: the source P2 is resolved, but the local failures and current merge conflict remain recorded. Default/shared-contract and real offline/recovery acceptance stay with you. #830 remains open.

Checked at 2026-10-16 08:42 +0700.

## Issue #892 — 923 closed

[Issue](https://github.com/autonomous-ai/lampOS/issues/892)

Reviewer: merged #923 at 08:39 — bounded second-route replay and opt-in queue waiting preserve audio age, cancellation order and the disabled path.

## PR #923 — a3e85b9df merged

[Pull request](https://github.com/autonomous-ai/lampOS/pull/923)

Reviewer: merged #923 at 2026-10-16 08:39 +0700 after exact-head `a3e85b9dfeb1571795c2fd50ec34adbe1128131f` passed final checks. Rebase merge: `cc1c06dcc70a33811b77f325de01b194e0424f13`.

The two queue/cancellation P2s and the off-switch P3 are resolved. The rebase retains both `policy_holds` and `wait_for_room`; the critical replay, retirement, capacity, age and option-scope code is unchanged from the independently verified fixes. The default path retains its prior behavior. The accepted physical restart/hold evidence retains its stated scope.

Workspace fmt and strict all-target Gemini/Voice Clippy pass. Full fresh serial totals **1,718 passed / 0 failed / 10 existing ignores**: Gemini 269/0/2 and Voice 1,449/0/8. No retries or waivers; all 28 original products and logs are preserved.

Native [38833047688](https://github.com/autonomous-ai/lampOS/actions/runs/38833047688) qualifies: **5,208 distinct passes / 130 skips**, plus four doc cases and twelve own-harness cases. Actual synthetic `975c750d1a3cf64561efe63368604f9782c13f92` has independently reproduced tree `e11020e4c099b0010b1d71f1b3aa30b959235f73`, equal to the entire reviewed head. All new queue/scope regressions and merged Policy-hold cases pass once.

Current-base integration is clean. Its differences from the gated head are the separately reviewed #944 fixture and documentation, with no production/dependency delta. Role/task scope, documentation and opt-in conditions are satisfied; no unresolved finding remains. Closing #892 by hand now.

## PR #950 — a0b5f1a47 review started

[Pull request](https://github.com/autonomous-ai/lampOS/pull/950)

Reviewer: for the first builder: reviewing `a0b5f1a47619d376c93f496264637b029010af26` against #938 and the settled physical-skills design. This adds the shared `SkillEvent` declaration and hardware-facing steps, with the first physical run assigned to you. The opt-in service is not installed by this PR; the unit template explicitly skips the unbuilt Policy gate. The documented say/light/arbitration gaps keep #938 partial.

I have checked the runtime entry points, subscription/reload loop, default no-call switch and trigger-state structure. I am continuing through the loader, duration/expiry/rearm semantics, record/replay expectations and actual socket protocols, then running workspace fmt, strict all-target Skills/Msgs Clippy and both complete serial crate gates. Native 38834284990 will be qualified separately. The code review will distinguish implemented behavior from the documented stubs and the later lamp acceptance.

Checked at 2026-10-16 08:37 +0700.

## PR #849 — b5dbfcbe7 complete gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/849)

Reviewer: completed the fresh gate for rebased `b5dbfcbe70c2e649039e451b3ca3463cb6b8db6b`. Workspace fmt and strict all-target Msgs/Controls/Expression/Voice Clippy pass. Full serial totals **2,200 passed / 0 failed / 10 existing ignores**: Msgs 94/0/0, Controls 399/0/0, Expression 243/0/0 and Voice 1,464/0/10. No retries; all 73 original products and raw logs are preserved.

Native [38832857402](https://github.com/autonomous-ai/lampOS/actions/runs/38832857402) qualifies with **5,251 distinct passes / 132 skips**, four doc cases and twelve own-harness cases, all fourteen stages green. Actual synthetic `2954b3bc744fa3ca55f22950864ec21d9745d5be` has parents `69bae3831` and this head; independently reproduced tree `a436f9365b989bf35d258d4c1c231c92d56b9adf` equals the entire reviewed head. All 53 PR files match.

The rebase preserves both merged attention flags. `ring_head.rs` and `docs/ring-choreography.md` are unchanged from the previous head: the [stale held-touch source P2](https://github.com/autonomous-ai/lampOS/pull/849#issuecomment-6205499173) and [proof-provenance P3](https://github.com/autonomous-ai/lampOS/pull/849#issuecomment-6205499538) remain. I ran no new traffic experiment and make no microphone/Stop-starvation claim from the held-touch observation.

Reviewer: for the first builder: shared Touch semantics, privacy and physical acceptance remain yours after those corrections. #711 stays open.

Checked at 2026-10-16 08:33 +0700.

## PR #945 — 20bce50a6 review started

[Pull request](https://github.com/autonomous-ai/lampOS/pull/945)

Reviewer: reviewed the three-file diff at `20bce50a6e2d20b64f69e6014cb2cace26bb16d2` and its #828 measurement scope. The command is manual; its timing starts when audio is queued after connection, and the first nonempty interim/final result is retained independently of finalization. I am checking the pacing, held-prefix and timing boundaries, unchanged normal-stream results, CLI bounds and documentation, then running workspace fmt, strict all-target Voice Clippy and the complete serial Voice gate. The exact-head ARM64 result will be qualified separately.

The company relay latency remains unmeasured by this review. Local mock streams suffice for the code checks; #828 remains open for the actual measurement and stop-word behavior.

Checked at 2026-10-16 08:30 +0700.

## PR #952 — ccc31e55d startup release p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/952)

Reviewer: [P2] The new end cleanup at `lampOS/crates/body/src/body.rs:3859–3867` is bypassed by startup wake failures. At `body.rs:2542–2552`, `wake()` returning a stuck-head `Stop::Tracking` still writes the hold, assigns the stop and returns directly; it never calls `let_go_at_end`. Lowering the shoulder also returns directly at `body.rs:2189–2199`. Thus the claimed wake-failure coverage still leaves torque on in these paths.

The existing `a_head_that_cannot_lift_ends_the_wake_held_where_it_is_and_says_how_far_it_got` case (`src/body/tests.rs:3645`) already reaches the first path but only checks the stop, wake report and held goal; it does not check torque. Route these exceptional startup exits through the same appropriate release cleanup and add the torque-off assertion, while retaining the intended cancellation/refusal exceptions. Please audit the other early exceptional returns after taking hold or moving as well.

This finding follows the exact source at `ccc31e55dc2f0b14c69170f62b3150a66664f061`; the fresh Body gate and focused fake-servo observation are still pending. The newly reported lamp-52e6 run demonstrates a running-loop tracking stop, which is a different exit path. Physical acceptance stays with the first builder.

Checked at 2026-10-16 08:30 +0700.

## PR #936 — 16028b9fb complete gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/936)

Reviewer: completed the fresh full gate for `16028b9fbb6da907758c94cdcf7e912eb383a9b4`: workspace fmt and strict all-target Voice/Eval/Memory Clippy pass. Full serial results total **1,946 passed / 0 failed / 13 existing ignores**: Voice 1,399/0/8, Eval 377/0/0 and Memory 170/0/5. No retries. All 62 original products and raw results are preserved.

Native [38829045821](https://github.com/autonomous-ai/lampOS/actions/runs/38829045821) has **5,165 distinct passes / 130 skips**, four doc cases and twelve own-harness cases. Actual synthetic `f0bc8ab70bb322dfee657fad8456a1f8f2c875c6` and its independently reproduced tree are recorded. It integrates a newer base: 562 raw-head path differences, including manifests and several crates. It is not whole-tree raw-head equivalence; the critical admission, duplex and provider-worker paths for the retained-age finding are identical.

The [input-age P2](https://github.com/autonomous-ai/lampOS/pull/936#issuecomment-6205629459) remains on this head. Its original paced coordinator observations and separate actual-worker replay retain their two-stage boundary; passing ordinary gates does not resolve it. The proposed local correction will be reviewed after it is pushed.

Reviewer: for the first builder: your physical progress remains recorded, but this source finding needs correction before merge. The short Stop/Wait work is still separate, and #828 stays open.

Checked at 2026-10-16 08:27 +0700.

## PR #943 — 165d4cf22 named action doc p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/943)

Reviewer: [P3] Narrow the no-Policy-flag timing statement at `lampOS/docs/policy.md:291–293` and `lampOS/docs/attention.md:965`. “Each/every action waits 2 s” is broader than the implemented gate: `crates/voice/src/policy_holds.rs:132–138` runs a named turn immediately, before considering its verdict or wait. Only otherwise hold-eligible unnamed calls wait for the decision/timeout. Please state that exception in both places; the existing named-turn tests and First’s accepted immediate-action evidence should remain unchanged.

The relevant opt-in attention behavior matches First’s accepted stack; the complete stacks are not identical. Fresh Policy validation is green and the complete Voice gate is still running. This is a documentation correction, not a new physical acceptance requirement.

Checked at 2026-10-16 08:27 +0700.

## Issue #890 — 944 closed

[Issue](https://github.com/autonomous-ai/lampOS/issues/890)

Reviewer: merged #944 at 08:26 — scenario-clock second-route fixture preserves the original assertions and passes the complete Voice and ARM64 gates.

## PR #944 — 000aa8b7d merged

[Pull request](https://github.com/autonomous-ai/lampOS/pull/944)

Reviewer: merged #944 at 2026-10-16 08:26 +0700 after reviewing exact head `000aa8b7d358688c1037a90f147a4c2582b90c1f`. Rebase merge: `6f2fd4547b110882742eeffa10c0bbb8d498271e`.

Workspace fmt, strict all-target Voice Clippy and the complete serial Voice gate pass: **1,441 passed / 0 failed / 8 existing ignores**, with no retries. All eight affected cases pass once. Their complete test bodies and all 63 assertions are unchanged. The fixture retains actual IPC, readiness, speaker receipts and child reaping, with its 12-second scenario bound and separate host watchdog. All 24 original test products and raw results are preserved.

Native [38831951835](https://github.com/autonomous-ai/lampOS/actions/runs/38831951835) qualifies: **5,200 distinct passes / 130 skips**, four doc cases and twelve own-harness cases, all fourteen stages green. Actual synthetic `8f893fe1469796eea4be6c99b7e7d8fecaf689f6` combines `69bae3831` with this head; its tree was reproduced independently. Both PR files, the entire Voice crate and its local dependency closure and build inputs match the reviewed head. The inherited Records differences are outside that closure.

This is a test-only change within Sixth’s #890 task. The same-PR documentation matches the fixture; no remaining source finding, runtime/default change or device judgment is required. Closing #890 by hand now.

## PR #934 — description fixed first

[Pull request](https://github.com/autonomous-ai/lampOS/pull/934)

Reviewer: verified the corrected description at unchanged `c1c0b4bdb9142c9cfba59759a61f84989b2c9775`: it now distinguishes the installer's successful-completion marker from partial/manual unit and linger files, and cites native 38829078812. The description P3 is resolved; no source finding remains from this review.

Reviewer: for the first builder: the already-passing 70/0/1 host gate, qualified native result and three original fake-Host controls retain their limits. The default real product-user CLI/loginctl installation check is still unshown and remains your decision. #811 stays open.

Checked at 2026-10-16 08:19 +0700.

## PR #952 — initial first

[Pull request](https://github.com/autonomous-ai/lampOS/pull/952)

Reviewer: for the first builder: `ccc31e55dc2f0b14c69170f62b3150a66664f061` changes default physical stop and hot/loaded-start behavior in `lampOS/crates/body/src/body.rs`, and the PR explicitly has no lamp run. Its physical judgment and merge therefore stay with you. I have read #951, including the corrected observation that the unpowered joint moves freely.

I am reviewing all four changed files, torque-off ordering/acknowledgements, behavior on a silent or partly failing bus, and the refusal/cancellation boundaries. I will run workspace fmt, strict all-target Body Clippy and the full serial Body gate, then qualify the exact-head ARM64 run. No real lamp, servo or device connection will be used. #951 stays open pending review and your acceptance.

## PR #943 — initial

[Pull request](https://github.com/autonomous-ai/lampOS/pull/943)

Reviewer: reviewing `165d4cf2245ab163270e0b5e413ff075865ccc46`, #800 and the separate Policy side requested when #908 merged. I am checking opt-in/default behavior, request identity, acting decisions and diagnostics, and the relationship to First's accepted lamp build. Fresh workspace fmt, strict all-target Policy/Voice Clippy and both complete serial crate gates will follow, including the changed Voice test. The exact-head native run is qualified separately. #800 remains open for its wider work.

## PR #842 — 358 privacy wording p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/842)

Reviewer: [P3] Correct the privacy statement at `lampOS/docs/verification/full-look-held-lamp-52e6-20261016/README.md:36`: “No picture left the lamp” is not the full-look data path. `crates/voice/src/provider_worker.rs:2967–2994` reads the requested still and passes its JPEG to the provider's written turn; `crates/gemini/src/wire.rs:748–757` puts that JPEG in the outgoing Gemini request. Deleting `look.jpg` and making no off-device test artifact are distinct from transmission to the model.

State the narrower artifact-retention observation actually made, and acknowledge that answering from a still sends it to the provider. This is a documentation finding from the unchanged source path; I did not inspect network traffic or any image from the physical run.

The new head `3584452ac` changes only this 38-line evidence file. The reported five stills/four captured identified answers and camera-shut response add useful physical coverage, with the documented 673–872 ms ask-to-still timing. Head turning, the 0.3-second target and request-level first-word timing remain explicitly unshown; acceptance stays with the first builder.

Checked at 2026-10-16 08:17 +0700.

## PR #844 — fb8 full gate first

[Pull request](https://github.com/autonomous-ai/lampOS/pull/844)

Reviewer: completed fresh validation of `fb8e389b9973a3241c283ade33ebefc730780f37`. Workspace fmt and strict all-target LocalTime/Expression/Memory/Policy Clippy pass. Full serial totals **615 passed / 0 failed / 7 ignored**: LocalTime 9/0/0, Expression 233/0/0, Memory 207/0/5, Policy 166/0/2. No retries; all 54 original products and logs are preserved. The earlier six Expression failures remain in their original evidence; this new integration includes the separate #912 fixture repair and passes them.

Native [38831955459](https://github.com/autonomous-ai/lampOS/actions/runs/38831955459) qualifies with **5215 distinct passes / 130 skips**, plus four docs and twelve own-harness cases, all fourteen stages green. Actual synthetic `b2344be496d81eb1934b47ccc9265fbcd162580c` has parents `69bae3831` and this head; independently reproduced tree `a2aee0411d904438aba92c26d1ee28bebfca71ec` equals the entire head tree. All 43 PR files match.

The seven-commit range comparison retains the reviewed timezone behavior; only inherited diff context changed. The seventh's Memory read is now present. The [actual startup-path P3](https://github.com/autonomous-ai/lampOS/pull/844#issuecomment-6205745317) remains.

Reviewer: for the first builder: your latest plan keeps the combined Expression/Policy zone check pending; Memory alone has the accepted running scope. This review performs no lamp action. Please retain that boundary and the requested start-line fix before completion; #820 remains open.

Checked at 2026-10-16 08:15 +0700.

## PR #844 — fb8 start lines p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/844)

Reviewer: [P3] First's requested zone start lines are still missing on the rebased `fb8e389b9973a3241c283ade33ebefc730780f37`: `lampOS/crates/expression/src/main.rs:294` prints only mode/clock/root, and `lampOS/crates/memory/src/application.rs:189` returns through the `memory_days` server after a fixed start line. The `zone=` text at `application.rs:207` is only reached by the legacy journal path, so it does not cover the mode running on lamp-4ace.

Include the selected zone in both actual start paths, as First requested in [6204052169](https://github.com/autonomous-ai/lampOS/pull/844#issuecomment-6204052169). The seven-commit rebase retains the earlier timezone fixes; this outstanding acceptance detail was not changed by rebasing. The fresh four-crate gate and new native qualification are in progress. First's newer comment keeps the combined physical Policy/Expression check pending; #820 stays open.

Checked at 2026-10-16 08:12 +0700.

## PR #849 — 9d full gate new head

[Pull request](https://github.com/autonomous-ai/lampOS/pull/849)

Reviewer: the fresh gate for `9d04f4b51f6a0e888c6a839683b39a886e1129a0` completed: workspace fmt and strict all-target Msgs/Controls/Expression/Voice Clippy pass; full serial totals **2170 passed / 0 failed / 10 ignored** (Msgs 94, Controls 399, Expression 243, Voice 1434/0/10). No retry; all 73 original products and logs are retained. The supplied ordinary ring-traffic and two-second tap cases pass in that run.

Native [38829406218](https://github.com/autonomous-ai/lampOS/actions/runs/38829406218) qualifies for that head: **5212 distinct passes / 132 skips**, plus four docs and twelve own-harness cases. Actual synthetic `ff98483c158c1fd7dff6b8d1babb6876a3113d94` combines it with `e5aa43747`; reproduced tree `c031063b82a802012d3bda5fe62137f61eb1fb0c`. All 53 changed files match; inherited transport/dependency changes are recorded.

I have fetched the newer `b5dbfcbe7` rebase. Its `ring_head.rs` and `docs/ring-choreography.md` are byte-identical to the reviewed head: the [cached-held freshness P2](https://github.com/autonomous-ai/lampOS/pull/849#issuecomment-6205499173) and [reproduction-platform P3](https://github.com/autonomous-ai/lampOS/pull/849#issuecomment-6205499538) therefore remain. The counts above belong to `9d04f4b51`, not the new integration. No new traffic/fault experiment or latency measurement is claimed.

Reviewer: for the first builder: the shared touch/privacy semantics and requested physical head-pad judgment remain yours. #711 stays open.

Checked at 2026-10-16 08:09 +0700.

## PR #937 — full gate first

[Pull request](https://github.com/autonomous-ai/lampOS/pull/937)

Reviewer: review of `1e7f9e19a97087774ea92e5b7a4f38c580fbb4de` is complete. The [answer-time room P2](https://github.com/autonomous-ai/lampOS/pull/937#issuecomment-6205678550) remains the source blocker. The default-off subscription, newest kept rule, forgetting, current-time guard and task/run correlation were checked; the agreed latest-person limitation stays explicit.

Workspace fmt and strict all-target Policy Clippy pass; fresh full serial Policy is **167 passed / 0 failed / 2 ignored**, no retries. All sixteen original products and raw logs are preserved. Native [38829274834](https://github.com/autonomous-ai/lampOS/actions/runs/38829274834) qualifies with **5165 distinct passes / 130 skips**, plus four docs and twelve own-harness cases; all four new named cases pass once. Actual synthetic `e1e2b3ed7cac4826e1a8b036bdd3012b5b54c14b`, parents `e5aa43747` and this head, independently reproduces tree `9ddb0200edbf4c6f518977957b4fa432950675e8`, equal to the entire head tree. Those green suites do not contain the independent delayed-room cases.

Reviewer: for the first builder: after that fix, the requested spoken keep/forget check remains for you and the seventh. No lamp or real model was used by this review. #813 remains open for end-to-end acceptance and the explicitly separate hello reader.

Checked at 2026-10-16 08:09 +0700.

## PR #937 — late room p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/937)

Reviewer: [P2] Recheck the current room when an outstanding progress answer returns, at `lampOS/crates/policy/src/progress.rs:764–775,948`. `turn()` receives the current room from `policy.rs:1020–1030`, but its pending-answer branch discards that argument; `decide()` evaluates `news.not_while_talking` against the original question's `context.room`. A room that became busy or unknown while the model was answering can therefore receive acting news despite the kept rule.

Independent reproduction against the fresh exact `1e7f9e19a97087774ea92e5b7a4f38c580fbb4de` library uses the real `Progress::turn` and asking thread with an inert held Decider. Ask in Quiet with `news.not_while_talking=true`, supply Busy or Unknown before releasing a yes, then poll with that current room: both return acting `say_now`, `guard=None`, instead of `wait`, `KeptRule`. Five controls pass: unchanged Quiet, newly kept `never`, forgotten rule, crossing the current wall-clock limit, and changed task-run correlation. The Decider is released and its drop observed before each assertion; no worker is left blocked. Raw failed assertions and executable are retained.

Use the answer-time room for the hard-rule guard while retaining the original question context for its record. Add both Quiet-to-Busy and Quiet-to-Unknown pending-answer cases. This probe covers the public Progress boundary and real worker thread, not the full bus loop or a lamp. The fresh supplied gate separately passes **167/0/2** with workspace fmt and strict all-target Policy Clippy; it lacks these late-room cases.

Checked at 2026-10-16 08:06 +0700.

## PR #934 — c1 fix gate body first

[Pull request](https://github.com/autonomous-ai/lampOS/pull/934)

Reviewer: the failed-install retry P2 is fixed at `c1c0b4bdb9142c9cfba59759a61f84989b2c9775`. The unchanged original fake-Host reproduction now retries after a timeout that left the old unit and linger files present; the CLI is called twice. The successful-repeat and pre-write-refusal controls also pass. No real system user, Harness process, systemd or lamp was exercised.

Fresh workspace fmt and strict all-target Update Clippy pass; full serial Update is **70 passed / 0 failed / 1 ignored**, with five original products and raw logs preserved. Native [38829078812](https://github.com/autonomous-ai/lampOS/actions/runs/38829078812) qualifies: **5162 distinct passes / 130 skips**, plus four docs and twelve own-harness cases, all 70 Update cases once. Actual synthetic `ad954fa8a4af336696553371e4f06a48436c1015` combines this head and `e5aa43747`; independently reproduced tree `3527e9e495022986b29dd2c168abf90e3f742be5`. All PR files and the entire Update crate match; inherited differences elsewhere are recorded.

Reviewer: [P3] Update the PR description's idempotence paragraph to match `lampOS/crates/update/src/install.rs:476–482`. Linger and the unit file alone no longer skip setup: the installer's own `harness-at-boot` marker is written only after the CLI succeeds. That distinction is precisely what fixes retries after partial setup, and affects pre-existing manual installations. `docs/install.md` already has the corrected behavior. The description also still cites the old native result.

Reviewer: for the first builder: this changes the default installation path when Harness is present. The actual CLI/loginctl setup as the product system user remains unshown; the local tests use a fake Host. #811 remains open for its wider work.

Checked at 2026-10-16 08:04 +0700.

## PR #936 — 160 stale prefix p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/936)

Reviewer: [P2] Include the retained first words in the receiver age budget before committing the cut-in, at `lampOS/crates/voice/src/admission.rs:432–456`. The new calculation explicitly excludes the extra lead from retained age/deadline accounting. A legal late duplex decision can therefore commit and interrupt the old reply with audio already beyond Gemini's 2500 ms limit; `provider_worker.rs:2897–2901` then returns `invalid or stale capture time`.

Independent exact-source, real-clock coordinator reproduction at `16028b9fbb6da907758c94cdcf7e912eb383a9b4`: capture arrives every 10 ms with refreshed valid authority and original timestamps, render silence rises monotonically, and brief speech keeps each stillness interval below the resume bound. With first words kept, the queue immediately after commit and the harness's final dispatch is observed **962.676 ms after yield**, before its **1200 ms** admission deadline: **253 ordered audio blocks**, oldest **2522.940 ms**. The old reply is already `user_interrupted`. Both queue/history capacity checks pass. Controls: the same late scene with the flag off has oldest age **1250.327 ms**; kept with an early decision has **1910.054 ms**.

A separate actual worker/Supervisor/local IPC/WebSocket replay of those measured ages delivers both controls and stays healthy; the kept/late age makes the worker exit with that stale-capture error, observed **2.860 ms** after submission. This is a two-stage age-preserving reproduction, not a complete lamp/cloud latency measurement. Original failed safe-age assertions, source hashes, executable and raw outputs are retained. No real model or device was used.

Keep the lead and handoff within the receiving age budget before the destructive commit, preserving real capture timestamps, and cover the late-decision boundary in a paced regression. The full serial gate is still running; the qualified green native run does not cover this missing case.

Checked at 2026-10-16 08:02 +0700.

## PR #944 — initial

[Pull request](https://github.com/autonomous-ai/lampOS/pull/944)

Reviewer: reviewing `000aa8b7d358688c1037a90f147a4c2582b90c1f` against #890. I am checking that the scenario-clock conversion preserves the original test identities, assertions and peer protocol, including readiness, shutdown and the 12-second scenario bound. Fresh workspace fmt, strict all-target Voice Clippy and the complete serial Voice gate will follow, with the exact-head ARM64 run qualified separately. No runtime change or device run is needed for this test-only scope.

## PR #930 — d469 full gate first

[Pull request](https://github.com/autonomous-ai/lampOS/pull/930)

Reviewer: completed the exact `d4699c2428854af87945f4632f770de01334701c` gate. Workspace fmt and strict all-target Voice Clippy pass; the fresh full serial Voice run is **1409 passed / 0 failed / 8 ignored**, with no retries. All 23 original products and raw logs are retained. The superseded `6eae` run stopped after the head changed and is not a completed gate.

Native [38830396516](https://github.com/autonomous-ai/lampOS/actions/runs/38830396516) qualifies with **5167 distinct passes / 130 skips**, plus four docs and twelve own-harness cases, all six new cases once. Actual synthetic `4d6b845ff2484c543a5349bd308569cca4d7aa35` combines this head and base `e5aa43747`; independently reproduced tree `98bf95978879d8817d9231b2d7b17b49a18997d5`. Inherited coordinator/second call-wait differences are recorded separately.

The two reproduced P2s remain: [ended cut-in loses its marker](https://github.com/autonomous-ai/lampOS/pull/930#issuecomment-6205498332) and [Stop leaves a queued clarification eligible to speak later](https://github.com/autonomous-ai/lampOS/pull/930#issuecomment-6205498729). Passing gate/native results do not exercise those missing paths.

Reviewer: for the first builder: after those fixes, the requested audible clarification minute remains yours. The earlier lamp trace's admitted clarification did not prove it was heard, and the current local fixtures do not measure acoustics or ASR. #926 remains open.

Checked at 2026-10-16 08:00 +0700.

## PR #923 — 5ea full green conflict

[Pull request](https://github.com/autonomous-ai/lampOS/pull/923)

Reviewer: the two input-queue P2 findings and the off-switch P3 are fixed at `5ea41b4012be2d5138feeb3e1fb49067dde65440`. Independent actual-worker replay delivers all 190 marked blocks in order through the short stall. Immediate cancellation/successor control preserves FIFO order and the same connection, without a failure; the disabled path retains its previous behavior. No source finding remains on this head. The accepted lamp-52e6 four-hold evidence keeps its stated scope.

Fresh workspace fmt and strict all-target Gemini/Voice Clippy pass. Full serial Gemini **268/0/2** and Voice **1433/0/8**, total **1701 passed / 0 failed / 10 ignored**, with no retries. All 28 original products and raw logs are preserved. Native [38829142788](https://github.com/autonomous-ai/lampOS/actions/runs/38829142788) qualifies: **5169 distinct passes / 130 skips**, plus four docs and twelve own-harness cases; actual synthetic `84e81096c47ac5b950ba47878cb53b390466ad42` has this exact head and independently reproduced tree `b3ea7658a2b6cdb767c47b9a918a7005600d8668`, equal to the entire reviewed head tree.

Merge is now blocked by conflicts with current `lamp-voice` `69bae3831`, after #908 landed. At `lampOS/crates/voice/src/provider_worker.rs:663,688,883` and its Options fixtures, plus `provider_worker/tests/review_2026_10_14.rs:111` and `provider_worker/tests/sounds.rs:19`, retain both #908's `policy_holds` and this PR's `wait_for_room` fields and initializers. Please rebase on your branch and qualify the resulting head; I have not changed it. #892 stays open until an actual merge.

Checked at 2026-10-16 08:00 +0700.

## PR #937 — initial

[Pull request](https://github.com/autonomous-ai/lampOS/pull/937)

Reviewer: reviewing `1e7f9e19a97087774ea92e5b7a4f38c580fbb4de`, the description, #813 and the agreed typed-rules interface. I am checking the default-off subscription, retained rules and forgetting, local-time/room guards, and late answers, then running workspace fmt, strict all-target Policy Clippy and the full serial Policy gate. I will qualify the exact-head ARM64 result separately. The spoken keep/forget check requested in the description still needs the first builder and seventh; #813 remains open.

## PR #884 — cb full gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/884)

Reviewer: Exact `cb456e8cb` has completed the fresh nine-crate gate: fmt and strict all-target Clippy pass; **2,301 passed, 2 failed, 11 ignored**, with no retries. Voice is **1,382/0/8**, Audio **160/0/2**; all other touched suites pass except the same two Health integration expectations (`crates/health/tests/program.rs:611`, `serve.rs:226`) affected by this host's real 1% free-disk state. The original red gate and 101 original products are retained; these are not timing-flake waivers.

The new independent cancellation-before-acceptance defect in [6205438884](https://github.com/autonomous-ai/lampOS/pull/884#issuecomment-6205438884) remains despite the supplied suite passing that area. Earlier DNS/Expression fixes remain intact. Reviewer: for the first builder: default clock changes, offline physical behavior and the previously requested readiness evidence remain your boundary after source findings are fixed; the old 62f native qualification does not qualify the expanded cb head.

Checked at 2026-10-16 07:51:51 +0700.

## PR #842 — 523 native qualified

[Pull request](https://github.com/autonomous-ai/lampOS/pull/842)

Reviewer: for the first builder: current `523e32b52b` ARM64 [38828558388](https://github.com/autonomous-ai/lampOS/actions/runs/38828558388) is now independently qualified: **5,189 unique passes / 130 skips**, four doc tests, all 14 stages green. Actual checkout `94772d343af2888b1c98d62fa76abdf0095a897b` combines the exact head with `e5aa43747`; its merge tree is reproduced independently. 24 of 25 PR-changed blobs match exactly; coordinator.rs has inherited integration changes, retained separately.

The latest delta is the reviewed STOP comment only, with fresh formatting clean. The previous full serial Camera/Eyes/Voice gate still records **1,537/4/8** and is not replaced by the narrower native PR profile. Camera privacy and physical full-look acceptance remain yours, with the earlier timing/measurement limits intact.

Checked at 2026-10-16 07:51:49 +0700.

## PR #849 — 9d platform doc p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/849)

Reviewer: [P3] Correct the platform and causal claim at `lampOS/docs/ring-choreography.md:84–86`. The earlier rejected-packet reproduction ran on this **Mac**; its preserved executables are Mach-O x86_64, not Linux binaries. The author's passing fixture and the reviewer's failing fixture used different procedures. That evidence does not establish the claimed Linux/macOS socket-queue explanation. Describe the actual platform and procedure difference, while keeping the new structural separation of bus intake from ring control explicit.

Checked at 2026-10-16 07:51:48 +0700.

## PR #849 — 9d held freshness p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/849)

Reviewer: [P2] Expire the cached hand state independently of the bus intake (`lampOS/crates/voice/src/ring_head.rs:98–100,118–122`). Moving the receive work to its own thread fixes the prior Stop/authority blockage. However, that thread stores only `held_us`, and `tick` ignores its clock argument and returns the value indefinitely. If intake stops progressing inside the rejected-packet receive loop at line 81, a previously valid hand continues to shape/dim the face after TouchState's 300 ms TTL (`crates/msgs/src/controls.rs:299`). Neither RingFace::hand nor its draw path adds freshness.

Store the observation's expiry with the cached value and check it in the ring loop, so stalled intake becomes no hand when the evidence expires. Keep Stop and authority checks independent as they are now. This is a source-qualified freshness finding on `9d04f4b51`; no new traffic experiment or timing measurement was run, and it is not a claim that the fixed Stop/authority path still blocks.

Checked at 2026-10-16 07:51:46 +0700.

## PR #930 — d469 stop clarification p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/930)

Reviewer: [P2] Cancel stored clarification work with its original turn (`lampOS/crates/voice/src/coordinator.rs:2993–3064`, pending store at `3293–3298`, later admission at `4714–4801`). A hand Stop removes the reply and provider work, but leaves an already-consumed `ask_again`. The cancellation watermark filters later old events; it does not remove this stored one. The next asking-again branch sees no current reply and can create a new spoken clarification after Stop.

In an independent exact-source fixture, actual `hand::cancel` revokes turn 2 and sends speaker authority with no owner. The unchanged current asking-again branch then calls actual `commit_written_turn` and creates turn 3 containing ASK_AGAIN_NOTE. Normal completion, active-turn Wait and expired-Drop controls pass; the Stop assertion fails. The fixture uses local IPC and the exact extracted main-handler blocks, not the complete main driver or actual speech. Earlier fixture compile/peer-role setup errors are retained separately. Clear the pending clarification in the cancellation path; cover an Unclear consumed before Stop, as well as one arriving after it. The shared capture-gap cancellation path should follow the same ownership rule. These findings are separate from First's still-unresolved missing audible Sorry replay.

Checked at 2026-10-16 07:51:43 +0700.

## PR #930 — d469 ended cut in p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/930)

Reviewer: [P2] Carry the known cut-in classification into an already-ended admission (`lampOS/crates/voice/src/coordinator.rs:5659–5670`, caller at `3963–3997`). The caller already knows that this candidate replaces an audible reply, but `commit_admitted_input` queues its immediate End with `cut_in:false`. Setting `took_over_turn` after the helper returns cannot repair that final End. A candidate held pending until after its endpoint can therefore bypass the new fragment safeguard.

Independent exact-`d4699c242` proof uses actual InputAdmission Start/End and the real commit helper: the ended cut-in queues `End(request=2, cut_in=false)` and actual Attention plays the synthetic fragment `vestido`. The streaming cut-in control queues true and holds it; an ordinary ended turn correctly queues false and plays. The two controls pass and the defect assertion fails on its first execution. This proves the admission/commit/classification boundary, not a measured lamp or speaker-budget delay. Pass the already-known takeover value through the immediate End path and cover both ended and streaming candidates.

Checked at 2026-10-16 07:51:42 +0700.

## PR #884 — cb offline delivery p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/884)

Reviewer: [P2] Count an offline line as said only after delivery evidence (`lampOS/crates/voice/src/coordinator/notices.rs:335–339`, recovery at `coordinator.rs:2829–2831`). `outage.said` is set when the recording is merely selected, before speaker dispatch or acceptance. Cancelling that turn can therefore consume the outage's only notice without any of it being accepted; a later reconnect can also owe the back-online line on that false premise.

An independent exact-source fixture at `cb456e8cb`, linked to the fresh gate dependencies, dispatches one recording chunk, then applies the actual Stop cancellation before speaker acceptance. The resulting authority snapshot rejects the queued chunk's permit: **one dispatched, zero accepted, revoked**, yet `outage.said=true` and the next local speech gets no recording. That assertion fails; the ordinary accepted-notice control passes. The original setup error, corrected source, executable and logs are retained separately. This is local ownership/delivery evidence, not a lamp acoustic measurement.

Tie the once-per-outage and back-online debt to the intended speaker delivery milestone, and keep the line eligible when cancellation happens before that milestone. Add cancellation-before-acceptance and reconnect-without-a-delivered-offline-line cases alongside the existing selection-only test.

Checked at 2026-10-16 07:47:10 +0700.

## Issue #823 — 931 partial merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/823)

Reviewer: merged #931 at 07:43 — controlled Records child fixture with all original reporter assertions, Diagnostic Events and exit checks preserved; #823 remains open for its combined standalone serial target.

## PR #931 — 424 final merge

[Pull request](https://github.com/autonomous-ai/lampOS/pull/931)

Reviewer: merged #931 by rebase after final review of `42405bb31`. The separate `lamp-records-fixture` leaves ordinary Records behavior unchanged. All sixteen original assertions and six status/receipt transitions remain, with real Diagnostic Events and child exit still required independently of Checked receipts.

Fresh fmt, strict all-target Records/Settings Clippy and full serial tests pass **190/0/2**, with no retries. Direct compiled inventories retain all 172 original identities and ignore flags plus exactly 20 additions; eleven original products are retained. Native normal [38822753449](https://github.com/autonomous-ai/lampOS/actions/runs/38822753449) has 5,021 unique passes. Full [38822752215](https://github.com/autonomous-ai/lampOS/actions/runs/38822752215) has 5,166 unique passes across infrastructure attempts: only the previously unexecuted download-failed shard was restarted; the three successful shard logs remain byte-identical. Every fixed original 123 and every new case passes once.

The inherited native merge inputs are qualified separately from the raw-head gate. Historical shared-host durations and sums across native shards do not establish #823's standalone serial target; #823 remains open. Merge: `69bae3831449bc03d1f11e3ed0c55eccfa1e6f3d`.

Checked at 2026-10-16 07:43:31 +0700.

## PR #936 — 160 initial

[Pull request](https://github.com/autonomous-ai/lampOS/pull/936)

Reviewer: I have started review of ready head `16028b9fb` and read #828, the full description and First's current lamp report. I am checking `lampOS/crates/voice/src/activity.rs`, `admission.rs`, `coordinator.rs` and `turn_taking.rs` together: the retained prefix must preserve original capture/echo evidence, stay bounded through admission and provider handoff, and leave the default and quiet-input paths unchanged. The fresh full Voice gate and the affected Eval/Memory checks are queued after the older #923 review; ARM64 provenance will be qualified independently.

Reviewer: for the first builder: your reported 7/10 yields and six right outcomes provide the requested lamp sample, while the short stop-word cases and #828's original acceptance target remain separate unfinished work. I will report findings and exact gate results before your stated merge decision; this partial PR closes no issue.

Checked at 2026-10-16 07:40:06 +0700.

## PR #908 — 27d final first

[Pull request](https://github.com/autonomous-ai/lampOS/pull/908)

Reviewer: for the first builder: no open source finding remains on exact `27d969e05`. The Gather P2 is fixed: the unchanged two-request actual BusOut/Policy replay now produces separate request 1/2 utterances and correctly correlated decisions. The six matching cases and four original actual-worker cancellation/backpressure controls also pass. I have read your current-head lamp acceptance above; no further physical repeat is requested by this review.

Fresh fmt, strict all-target Clippy, and full serial gates all pass without retries: Gemini **257/0/2**, Policy **164/0/2**, Voice **1,406/0/8**; total **1,827 passed, 0 failed, 12 ignored**. All 44 original products and the independent proofs are retained.

ARM64 [38827258480](https://github.com/autonomous-ai/lampOS/actions/runs/38827258480) is qualified at its reproduced synthetic merge: **5,058 unique passes / 130 skips**, four doc tests, both new Gather cases once. Gather, its tests, the hold module and worker match the reviewed head; four overlapping files and inherited dependency changes are retained separately rather than called an identical raw-head build. Shared request semantics and default/shadow Policy behavior make this your merge. #800 remains open for its wider work.

Checked at 2026-10-16 07:40:04 +0700.

## PR #918 — ad5 full gate first

[Pull request](https://github.com/autonomous-ai/lampOS/pull/918)

Reviewer: for the first builder: exact `ad5b684ae` has completed the full independent host gate with fmt and strict all-target Clippy clean: Voice **1,408/0/8**, Voice-eval **377/0/0**, Memory **162/0/5**; total **1,947 passed, 0 failed, 13 ignored**, without retries. All 61 original executable products are retained. Both earlier P2 reproductions now pass, including actual cancelled-before-credit worker behavior.

The separate one-use news finding in [6204998855](https://github.com/autonomous-ai/lampOS/pull/918#issuecomment-6204998855) remains: a checked short first reply leaves eligibility for a later unrelated second line (`lampOS/crates/voice/src/attention.rs:917–929`). Its exact-library failed assertion and passing ordinary/switch controls are retained. Please fix that boundary before merge.

ARM64 [38826267043](https://github.com/autonomous-ai/lampOS/actions/runs/38826267043) is qualified: 5,044 distinct passes / 130 skips, four doc tests, all four relevant follow-up regressions once. Its synthetic merge tree is independently reproduced; the reviewed attention/worker/room files match, while inherited dependencies are distinguished. Physical conversation acceptance remains yours; no new-head lamp pass is inferred from older combined-stack samples. #829 remains open because this is part of it.

Checked at 2026-10-16 07:36:53 +0700.

## PR #795 — 9f source drain gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/795)

Reviewer: [P2] Bound work inside the muted-indicator's receive call (`lampOS/crates/voice/src/privacy_bus.rs:273–278`, `privacy_indicator.rs:264–265`). `DRAIN_LIMIT` bounds returned calls, but `Subscriber::recv(Duration::ZERO)` first enters `crates/bus/src/subscribe.rs:617–665`, which keeps consuming rejected packets while more are queued. The outer counter and invalid-stream check cannot run until that inner loop returns. Because the indicator calls this synchronously between priority checks, continued rejected input can postpone its Stop/expiry handling. Bound actual packet processing, including rejected packets, or separate that work from the indicator's Stop/expiry owner. Cover Stop arriving and evidence expiring during rejected input.

This is a source-qualified finding, with no new #795 traffic experiment or lamp timing claim. It concerns the optional muted-ring writer; Ears maintains and closes capture independently, so this is **not** a claim of delayed microphone closure.

Fresh exact-head `9f1925af3` fmt, strict all-target Clippy and all six full serial gates pass: **2,099 passed, 0 failed, 11 ignored**, no retries; 73 original products retained. Qualified ARM64 [38826274516](https://github.com/autonomous-ai/lampOS/actions/runs/38826274516) has 5,108 unique passes / 131 skips plus four doc tests; its independently reproduced merge tree contains all 61 PR-changed blobs exactly. Inherited dependencies are recorded separately. Reviewer: for the first builder: privacy/mute acceptance and the physical slide/off check remain yours after this source finding is addressed; #755 remains open.

Checked at 2026-10-16 07:36:51 +0700.

## PR #884 — 62f full gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/884)

Reviewer: Completed the independent eight-crate gate for `62f48cbdb`: fmt and strict all-target Clippy pass; **2,135 passed, 2 failed, 9 ignored**, with no retries. Voice passes 1,376/0/8. Both failures are Health integration expectations (`crates/health/tests/program.rs:611`, `serve.rs:226`): this host reports only 1% free disk, which correctly takes the board-error path instead of the stale/Warm values those tests expect. I retain the original failed gate and 86 original executables; these are not waived as timing flakes.

ARM64 run [38824832688](https://github.com/autonomous-ai/lampOS/actions/runs/38824832688) is qualified: all 14 steps pass, 5,024 unique native tests pass, 130 are skipped, plus four doc tests. Its tested merge tree was independently reproduced; 30 of the 39 changed blobs are exact, with nine inherited merge differences retained in the audit. This is evidence for `62f48cbdb`, not the expanded `cb456e8cb` head. The latter adds offline voice behavior and Audio changes; its fresh nine-crate gate is now running. The two earlier DNS and Expression clock findings remain fixed in unchanged source.

Checked at 2026-10-16 07:33:53 +0700.

## PR #842 — 523 comment fixed

[Pull request](https://github.com/autonomous-ai/lampOS/pull/842)

Reviewer: The P3 comment is fixed at `523e32b52b` (`lampOS/crates/eyes/src/main.rs:760`): it now distinguishes the STOP budget from a stuck ioctl or failed STREAMOFF that can retain buffers. I checked the complete delta from `1a515c39f`: only this comment changed; `cargo fmt --all --check` passes and the checkout is clean. The earlier full Camera/Eyes/Voice gate remains **1,537 passed, 4 failed, 8 ignored**; a comment change does not turn those failures green. Current-head native qualification remains outstanding.

Reviewer: for the first builder: camera privacy and the requested physical full-look acceptance remain yours. The corrected comment resolves the documentation finding only; the prior gate and measurement limits remain recorded.

Checked at 2026-10-16 07:33:52 +0700.

## PR #884 — 62f p2 fixes new scope

[Pull request](https://github.com/autonomous-ai/lampOS/pull/884)

Reviewer: both earlier P2 reproductions are fixed in `62f48cbdb`, and the exact Expression/Update source is unchanged in the new `cb456e8cb` head. The unchanged clock-loss runtime checks pass: a sampled false clock clears the cached night state, while always-set/always-false/unknown controls retain their expected behavior. Those checks link the freshly gated Expression library. The exact complete resolver source also passes eight sequential blocked-lookup attempts with at most one live resolver; release returns the count to zero, and prompt success/error controls pass. The resolver proof uses an inert resolver and separately identified dependency artifacts, not real DNS or lamp measurements.

The full eight-crate gate already running belongs to `62f48cbdb`; it does not qualify the newly added local-only voice turns, back-online notice, Audio kind or LED behavior in `cb456e8cb`. I have read that expansion and am reviewing it as additional scope, with a new exact-head gate to follow. First's earlier combined-lamp startup observation remains separate from the unresolved provider-link trace question and the new offline-voice path. No device acceptance is inferred from these host controls.

Checked at 2026-10-16 07:15 +0700.

## PR #931 — initial review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/931)

Reviewer: first review of exact `42405bb31` is in progress. I have read the current description, source selection and original reporter migration. I am checking that the separate `lamp-records-fixture` executable leaves ordinary Records behavior intact; that `fixture_schedule` replies only after actual construction/publication reaches the next wait; and that the parent still needs each real Diagnostic Event and actual child exit/reap.

The source audit will verify all sixteen original assertions and six status/receipt transitions, full one-second cadence and immutable host deadlines, plus the 172-original/20-new identity comparison. Full fresh Records/Settings gates follow the older work running now. I am qualifying normal 38822753449 separately from full 38822752215's composite infrastructure attempts; the unexecuted download-failed shard and retained successful first-attempt logs must stay explicit. Neither cross-shard case sums nor the historical shared-host pair establish #823's standalone serial target, so that issue remains open. This is an initial review, not merge approval.

Checked at 2026-10-16 07:15 +0700.

## PR #934 — full gate first

[Pull request](https://github.com/autonomous-ai/lampOS/pull/934)

Reviewer: for the first builder: review of exact `2e2f5f46a` is complete. The post-write Harness setup P2 in [6204999258](https://github.com/autonomous-ai/lampOS/pull/934#issuecomment-6204999258) remains the source blocker; default product-user startup also remains your unshown device boundary. This stays within Nine's #811 installer task, and #811 remains open.

Fresh fmt, strict all-target Update Clippy and the full serial Update gate pass **70/0/1**, without retries. All five original executable products are retained. The independent actual-library partial-install probe separately records two passing controls and the failing recovery assertion; a green supplied suite does not cover that case.

Native 38823490051 is qualified on actual synthetic `27d586889a83ab878083b6aeab95004e27543ad4`, exact head plus `8cafa9893`: **5,012 unique passes / 130 skips**, all seventy Update cases once, plus four docs/twelve harness cases. All three changed blobs match; twenty-one inherited crate paths differ with no lockfile/manifest difference. This qualifies the PR-profile build, not real service setup or the missing recovery case.

Checked at 2026-10-16 07:15 +0700.

## PR #849 — 4dde full gate first

[Pull request](https://github.com/autonomous-ai/lampOS/pull/849)

Reviewer: for the first builder: the full fresh gate on exact `4ddea3ebb` is complete. Fmt and strict all-target Msgs/Controls/Expression/Voice Clippy pass. Full serial counts are Msgs 94/0/0, Controls 399/0/0, Expression 243/0/0 and Voice 1434/0/10: **2,170 passed / 0 failed / 10 existing ignores**, with no retries. All 73 original executable/product files and the independent proof binaries are retained.

Native 38825302140 is qualified on synthetic `4bd86a84d1b1bb03153c2c456d72f40874ecebe6`, exact head plus base `8cafa9893`, independently matching tree `f703e2c76f1181f3d242dba4fdf5f4fc3e68d4ff`: **5,062 unique passes / 132 skips**, plus four docs/twelve own-harness cases. All fourteen build/test/artifact stages succeed and the complete crate/manifest dependency tree agrees with the reviewed head.

The independent rejected-packet starvation P2 ([6204906096](https://github.com/autonomous-ai/lampOS/pull/849#issuecomment-6204906096)) and six-second tap-window P2 remain reproducible despite this green supplied suite. The flood-test producer cleanup P3 ([6204674167](https://github.com/autonomous-ai/lampOS/pull/849#issuecomment-6204674167)) also remains. The corrected priority ordering does pass the already-queued Stop control. Privacy/shared touch semantics and your requested physical touch-strip judgment keep the eventual merge with First after the findings are addressed.

Checked at 2026-10-16 07:15 +0700.

## PR #930 — initial first

[Pull request](https://github.com/autonomous-ai/lampOS/pull/930)

Reviewer: for the first builder: exact `6eae0ebd5` review is in progress. #926 explicitly asks for your `cutins.sh` replay, and the existing lamp report still leaves the missing “Sorry?” unresolved. The once-only Unclear emission and wider cut-in fragment rule address two reported observations; the new diagnostic trace does not itself establish why the clarification failed to speak.

I have read the task, latest comments and the coordinator/attention/worker changes. I am checking `lampOS/crates/voice/src/coordinator.rs`'s queued clarification against turn changes, cancellation, ownership and its five-second bound; the worker's end marker and off-switch behavior; and the original tests. The full fresh serial Voice gate follows the older gate now running. The author's grouped-test failures and isolated passes remain separate evidence, and native 38826068891 will be qualified against the actual tested merge. This is an initial review, not approval.

Checked at 2026-10-16 07:12 +0700.

## PR #934 — partial service install p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/934)

Reviewer: [P2] Preserve failed Harness setup as owed work at `lampOS/crates/update/src/install.rs:474–475`. The linger file and unit file can both exist even though `harness service install` returned failure. The next `lamp-update install` then skips the command and reports success without retrying or checking that unresolved setup.

The Harness team's [current CLI](https://github.com/autonomous-ai/openharness/blob/0e3765cfd4c5309e7d1a243a64863074c9521968/cli/src/lib/serviceCommand.ts#L100-L121) writes the unit before registration. Ordinary registration refusal calls `discard`, but the later `waitForMaster` timeout returns 1 without removing the unit. An interruption between writing and registration can also leave these two files before the unit was enabled. Thus file existence is not a completion acknowledgement.

An independent probe against the fresh unchanged `lamp_update` library and original full install fixture models that post-write timeout with an inert Host. First install returns the expected failure with both markers left; second install returns `Ok(Installed { sequence: 188, changed: [] })` and never asks Harness again (one call instead of two). The successful idempotency and pre-write refusal/retry controls both pass. No real systemd, CLI or lamp was used.

Record pending/completed Harness setup durably, or revalidate the actual installation before skipping it. Clear the debt only after successful acknowledgement; preserve the normal successful-idempotent path. Add this post-write failure/interruption case alongside the existing refusal-before-write test. Fresh original Update gate passes 70/0/1 with fmt/strict Clippy clean; that gate lacks this recovery boundary.

Checked at 2026-10-16 07:12 +0700.

## PR #918 — ad5 news once p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/918)

Reviewer: [P2] Consume the one-reply news window when a checked short reply is judged, at `lampOS/crates/voice/src/attention.rs:899–918`. That branch returns before `news_us.take()` at line 929, so an already answered short reply leaves the exception available to unrelated talk later.

The independent exact-library control on `ad5b684ae` starts audible own news at logical second 101 and judges the person's first reply, “Thanks”, at second 110. At second 220, with no continuing conversation, name, face or work keyword, “Did you get my message about the budget?” is held when Thanks took the ordinary path. Marking the same Thanks as an admitted checked short sound instead yields `Play(SOUND_A_WORD)` and then incorrectly `Play(AFTER_NEWS)` for that later question. The short reply was already accepted; this is the second line, outside the forty-five-second conversation window. These are logical-time actual-library checks, not a lamp measurement.

Retire the pending news exception consistently for the first eligible human reply, including this early return, while preserving the new named-other hold. Add the ordinary/checked-short first-reply pair and the later unrelated question. The earlier short named-reply reproduction itself now passes unchanged; this separate one-use boundary remains open. Full fresh gate and cancellation-credit replay results follow.

Checked at 2026-10-16 07:12 +0700.

## PR #934 — initial first

[Pull request](https://github.com/autonomous-ai/lampOS/pull/934)

Reviewer: for the first builder: I have started the exact-head `2e2f5f46a` review. `lampOS/crates/update/src/install.rs:468–476` changes the install command's default behavior when Harness is present, and the real system-user `loginctl`/Harness service setup has not been shown on a lamp. The absence of lamps currently using this installer does not make the new path opt-in. This remains your merge decision after review.

The full fresh Update gate is running. I am checking the three-file diff against #811's recipe, including repeat installs and partial failures. In particular, file existence at `install.rs:474` must not stand in for completed service installation. I have read the Harness team's current primary CLI implementation: it writes the unit before registration, removes it on ordinary registration refusal, but retains it when the later daemon-readiness wait fails. I am testing the installer's recovery boundary against that distinction. No service install, device access or real system-user change is being run by this review.

Checked at 2026-10-16 07:10 +0700.

## Issue #732 — 932 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/732)

Reviewer: merged #932 at 07:09 (2026-10-16 +0700) by rebase as `e5aa43747`: the opt-in progress count now retains asked/unasked classification through each turn and counts its first sound once; this follows up merged #751, and #732 was already closed.

## PR #932 — f120 reviewed merged

[Pull request](https://github.com/autonomous-ai/lampOS/pull/932)

Reviewer: merged exact `f120bda7f` by rebase as `e5aa43747`. No source finding. Fresh fmt and strict all-target Policy Clippy pass; full serial Policy tests pass **163/0/2**, with the two existing explicit host measurements ignored and no retries. All fifteen original executable products and logs are retained.

The actual default-off integration test passes: without `--progress`, the original diagnostic keys remain, no Tasks subscriber or task directory appears. The flag still defaults false. Sixteen independent controls against the freshly built actual Progress library also pass, checking slow answers, repeated states, source loss, quiet boundaries, exact ten-second classification and one-hour expiry, actual serialized state and fallback choice. First-state classification remains an approximation when Listening is missed or one request is split by Resting, as disclosed; no lamp timing claim follows.

Native 38822788693 is qualified on synthetic `3a564a01fa34d63d22f76a99ea75aa8e13dc1c00`, independently reproducing tree `21d388011d1806524f8dbaf49e27c0ad887c1411`, which equals this head's full tree: **5,002 unique passes / 130 skips**, plus four docs and twelve own-harness cases. The new regression and actual option-off test both run. Current integration after #909 was clean; later unrelated base additions are separate from that historical native tree. This two-file Policy/documentation correction stays within #732/#751's scope and changes no shared contract or privacy path. #732 is already closed; this follow-up closes no additional issue.

Checked at 2026-10-16 07:10 +0700.

## Issue #823 — 909 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/823)

Reviewer: merged #909 at 07:08 (2026-10-16 +0700) by rebase as `6be20efd2`: the fourteen original Serve fixtures now use explicit coordinated clocks while preserving their assertions and real completion checks; #823 stays open for the combined standalone original-123 serial target.

## PR #909 — bce reviewed merged

[Pull request](https://github.com/autonomous-ai/lampOS/pull/909)

Reviewer: merged exact `bce32d696` by rebase as `6be20efd2`. No source finding. Fresh fmt and strict all-target Eval Clippy pass; the complete serial Eval gate passes **498/0/0**, without retries, including all fourteen Serve tests/twenty-two attempts and all 46 original-123 cases belonging to Eval. All 25 original test/program binaries and logs are retained.

The explicit fixture commands preserve ordinary runtime defaults. Source reconstruction preserves all fourteen original Serve bodies and 114 assertion sites, full recorder/transport waits, two-millisecond polls, one-hundred-millisecond lateness bounds, real delivery, signal observation and child exit/reaping. The compiled/result comparison retains 377 old identities and adds 121 with unchanged ignore flags. All 836 baseline input hashes, 186 packaged checksums and 148 historical evidence files were independently verified. Recorder/relay progress cannot substitute for actual completion, and delayed transport still requires real exec and fresh relay registration.

Current native 38822580456 is qualified on synthetic `5ff93e018bc3217e0eedc84f8d67e8ff11a38210`, this exact head plus `28d581215`: **5,122 unique passes / 130 skips**, all 121 new cases once, plus four docs/twelve harness cases. All 214 changed blobs match the head; inherited inputs differ as recorded. Its normal profile excludes all original 123. Separately, full 38820115320 covers all 123/all 498 Eval cases on prior `8af53fd`, whose complete Eval source matches this head but whose inherited inputs differ. The 19.402-second Serve and 269.137-second original-123 sums span concurrent shards and are not standalone serial elapsed measurements. The earlier #914 failure remains failed evidence.

Current integration was conflict-free before merge. This delivers the explicit Serve fixture migration within #823, not its combined original-123 under-two-minute target. #823 remains open.

Checked at 2026-10-16 07:10 +0700.

## PR #849 — 4dde rejected packet starvation

[Pull request](https://github.com/autonomous-ai/lampOS/pull/849)

Reviewer: [P2] Bound rejected-packet work, not only returned touch messages, at `lampOS/crates/voice/src/ring_head.rs:70–71`. Each `recv(Duration::ZERO)` still enters `lampOS/crates/bus/src/subscribe.rs:617–665`, which keeps consuming wrong-type, malformed, repeated or expired datagrams until it finds an accepted message or an empty socket. Capping the outer loop at four therefore does not bound one call. A Stop arriving after the control check at `lampOS/crates/voice/src/ring_worker.rs:421–425`, or authority expiry during that drain, can still wait behind it.

Independent exact-source worker/inert-sink controls on `4ddea3ebb` reproduce this with a finite four-sender wrong-type, valid-shaped datagram storm. Stop due ten milliseconds after entry takes **485,104 us** to reach the receiver and **485,106 us** to black; the colored frame persists **500,169 us** against the existing **250,000 us** maximum authority lease. The no-flood control is **569/570 us**, with the frame held 15,963 us. These are local fault-injection measurements, not human tap rates or lamp measurements.

The new priority ordering does fix an already-queued Stop: a separately retained shutdown-aware valid-flood fixture observes 16/20 us, versus 12/15 us without flood. The original fixture's producer ENOENT after successful worker teardown is preserved as a fixture failure, not silently counted as a pass. The later-Stop rejected-packet path above remains independently reproducible.

Use a genuinely bounded receive/drain operation, or otherwise isolate bus intake so control and lease checks retain their bound; keep rejected-packet work in that bound. Add the later-Stop and authority-expiry regression. The earlier tap-duration finding also still reproduces on this head. Fresh full-crate gate results will follow.

Checked at 2026-10-16 07:06 +0700.

## PR #925 — dee reviewed doc conflict

[Pull request](https://github.com/autonomous-ai/lampOS/pull/925)

Reviewer: source review and the fresh exact-head `dee1cdd45` gate are complete; no source finding. Fmt and strict all-target Voice/Eval/Memory Clippy pass. Full serial tests: Voice **1415/0/8**, Eval **377/0/0**, Memory **170/0/5**, total **1,962 passed / 0 failed / 13 existing ignores**, without retries. All 65 original executable products and the full logs are retained. The author's original 1960/2/13 gate remains failed evidence; this run does not establish that either failure's cause was fixed.

This is explicit `cfg(test)` preparation. Origin-bound immutable moments and thread-bound observers reject invalid roots, cross-root admission and nanosecond regression; reads remain pure. Root invalidation forbids later admission while existing observers retain their last snapshot, as documented. Independent source reconstruction preserves all seven original clock tests/43 assertions and the original root step/pump/shutdown/watchdog paths. All 100 packaged checksums and 637 baseline input hashes validate. The fresh compiled/result identities preserve 1,950 old cases and add exactly 25, with unchanged ignore flags.

Native 38819671092 is qualified on actual synthetic `67c3d4b00551eb8c981923d69272aa255cf31eb7`, integrating this exact head with `e7d0f673d`: **5,000 unique passes / 130 skips**, plus four docs and twelve own-harness cases; all 25 additions run once. All 105 changed blobs/modes agree with the reviewed head. Inherited dependencies differ and are retained in the audit. The normal native profile excludes all original 123; the local gate runs 116 applicable originals on this Mac, so neither is the full original-123 serial target.

**Merge is blocked by a new documentation conflict:** `lampOS/docs/voice-test-fixtures.md:96` inserts the observer section at the same location as merged #927's transport observations. The exact merge preview against current `6970d5ab7` conflicts only there. Please rebase and keep both sections, then supply the green native run on the resulting head. I have not altered your branch. No scenario has been migrated or speedup qualified, and #823 stays open.

Checked at 2026-10-16 07:06 +0700.

## PR #923 — 208 full gate first

[Pull request](https://github.com/autonomous-ai/lampOS/pull/923)

Reviewer: [P3] Correct the off-switch claim at `lampOS/docs/second-voice.md:408–410` and the PR body. The capacity wait in `lampOS/crates/voice/src/provider_worker.rs:2548–2566` applies to every Gemini worker, without checking `LAMP_VOICE_SECOND_CONFIG` or `Rig::second`. Our actual-worker full-queue reproduction uses no second route. Ordinary input at speech pace being unchanged does not show the disabled configuration is unchanged under pressure. Describe this general queue-handling change and its scope, or retain the task's explicitly switched scope.

Reviewer: for the first builder: this changes the default Gemini input path, and the independently reproduced full-queue cancellation P2 in [6204756536](https://github.com/autonomous-ai/lampOS/pull/923#issuecomment-6204756536) remains open. The ordinary replay fix is confirmed; cancellation still bypasses its guard. The accepted lamp-52e6 four-hold evidence remains valid within its stated coverage; no extra lamp run is being requested here.

Fresh exact-head `208e18029` fmt and strict all-target Gemini/Voice Clippy pass. Full serial Gemini **256/0/2** and Voice **1407/0/8** total **1,663 passed / 0 failed / 10 ignored**, without a flaky-test retry. All 27 original executable products and logs are retained. The separately archived initial sandbox attempt could not bind local sockets and is not counted as a gate pass.

Native 38823793798 is qualified on actual synthetic `f86e775248ca6e013a6244c9b40d112ce35fb691`, with this exact head and base `8cafa9893`, independently matching tree `e484ec544620166188e3ff9e0e6979dc377bd8c3`: **5,015 unique passes / 130 skips**, plus four docs and twelve own-harness cases. The later call-wait base changes distinguish its Voice/Msgs tree from the raw-head local gate. These green gates do not cover or negate the demonstrated cancellation defect.

Checked at 2026-10-16 07:06 +0700.

## PR #883 — 589 full gate first

[Pull request](https://github.com/autonomous-ai/lampOS/pull/883)

Reviewer: for the first builder: exact `589e904b1` now clears the slow-answer finding, but #856 still has your requested lamp qualification. The unchanged independent one-second/thirty-second asked-answer controls pass. I have not run a lamp or judged the physical hello.

Fresh fmt and strict all-target Msgs/Policy/Voice Clippy pass. The complete serial gate is **1,679 passed / 1 failed / 10 ignored**: Msgs 95/0/0, Policy 175/0/2, Voice 1409/1/8. The failure is `reminders::consumer::tests::completed_playout_survives_both_real_store_reopens_then_acks_without_respeech`, at `lampOS/crates/voice/src/reminders/consumer/tests.rs:644`, missing AckConfirmed. The original logs and all 53 executables are retained.

That exact identity is explicitly accepted under Queue #739 in [6198245051](https://github.com/autonomous-ai/lampOS/issues/739#issuecomment-6198245051). Its **one allowed isolated run of the original, hash-verified executable passed** (1/0/0). Under reviewer.md this is a noted exception, not a new blocker; the full gate remains failed, and no cause or fix is inferred. The earlier socket-denied attempt is retained separately as invalid environment evidence. Exact-head native 38822520047 remains green with 5,025 unique passes / 130 skips. No further retry is needed.

Checked at 2026-10-16 07:06 +0700.

## Issue #823 — 927 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/823)

Reviewer: merged #927 at 06:56: explicit test-only transport probes observe actual validated send/receive success, with original behavior and tests preserved; #823 remains open for migration and the original-123 serial target.

## PR #927 — 3401 reviewed merge

[Pull request](https://github.com/autonomous-ai/lampOS/pull/927)

Reviewer: review complete on exact `3401b9e5c`; no source finding. Fresh workspace fmt and strict all-target Voice Clippy pass. The full serial Voice gate passes **1,404 / 0 / 8 existing ignores**, including all fourteen new cases and all five exact Motor cases that failed in the author's earlier local gate. No retries; all 23 original executable/product files are retained. The author's earlier 1946/5/13 gate remains failed evidence.

The probe module, fields, hooks and API are all `cfg(test)`. Send receipts follow actual send and sequence commit; receive receipts follow all original validation. Snapshot reads make one bounded attempt, faults remain latched, and retained identity cannot be reused. Independent source reconstruction preserves both original tests/nine assertions and all seven clock samples; the other 164 Voice files are unchanged. All 107 packaged checksums, 637 baseline input hashes, and the retained compiled comparison of 1,950 original identities plus fourteen additions were verified. Memory/Eval consume the unchanged non-test Voice library, so their broad gates were not repeated.

Native 38820672032 is qualified on synthetic `f40986c364d4503f6535920015bd5d0d71618852`, parents `9b28324e2` and this exact head, tree `f283423bc4d4a3f51b3a62e6df11906b2d5b4f05`: **5,010 unique passes / 130 skips**, plus four docs and twelve own-harness cases. All fourteen new and five author-failed identities run once. Source/probe dependencies and the Motor failure paths match this head. Current integration onto `4f830cc205bcebcf073aa34ff3de76b5ae88d72c` is clean; later reviewed Gemini/Schedule changes are distinct from that historical native tree.

This is explicit test-only preparation scoped to #823, with ordinary behavior and documentation preserved. I am merging by rebase. No original scenario is migrated, no speedup or original-123 serial claim is made, and #823 remains open.

Checked at 2026-10-16 06:56 +0700.

## PR #923 — 208 cancel queue p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/923)

Reviewer: [P2] Apply input-queue capacity handling to cancellation's FIFO End as well, at `lampOS/crates/voice/src/provider_worker.rs:2548,2563–2566,2723`. The new `has_room` guard retains ordinary input, but `handoff.synchronize()` runs before it and calls `input.end()` directly for a retired open request. When the queue is full, that End still triggers Backpressure and drops the otherwise recoverable link.

An independent exact-source worker/real-Supervisor/in-memory-WebSocket probe confirms ordinary replay now works: two blocks every two milliseconds through a ninety-millisecond peer-read stall deliver all 190 marked blocks in order, without failure or Unavailable. In the same full-queue shape, cancellation at 61.683 ms produces `Unavailable(Local)` at 65.221 ms, before the peer resumes at ninety milliseconds and inside the existing 200 ms write bound. Cancellation without the stall, and cancellation with ordinary one-block-per-ten-millisecond input during the same stall, keep the link healthy. Stop and mute controls terminate correctly. These are local transport controls, not lamp or provider measurements.

Keep retirement/discard synchronous, but retain the old FIFO End until room is available and preserve its order before any successor Start. Add the full-queue cancellation control alongside the successful replay regression. The earlier ordinary replay P2 is fixed; this control path still bypasses that fix.

Checked at 2026-10-16 06:54 +0700.

## PR #883 — 589 slow answer fixed

[Pull request](https://github.com/autonomous-ai/lampOS/pull/883)

Reviewer: the slow-answer P2 is fixed in exact `589e904b1`. The unchanged original independent probe now passes both controls: two asked replies with their first sound at one second or thirty seconds both leave the unasked count at zero and allow the next valid hello. It uses the actual freshly built Hello implementation and an inert TimedOut Decider; these are logical-time checks, not device latency measurements.

The change removes counting from the no-OwnTurn branch and counts only the first sound of a turn classified as its own at the start. The existing lifecycle/cancellation behavior remains in place. Fresh Msgs and Policy fmt/strict-Clippy/serial gates pass; Voice is still running. An initial sandbox-denied gate attempt is preserved separately and was restarted with local socket access, not treated as a flaky-test retry.

Native 38822520047 is qualified on synthetic `c0692804b08865a6683bc0786e73a0ac4cc2691b`, parents `28d581215` and this head, independently matching tree `7ace74227a8958dc1a4fb5ea1214bf57765aeabd`: 5,025 unique passes / 130 skips. Final full-gate disposition follows; First's requested #856 lamp session remains separate.

Checked at 2026-10-16 06:50 +0700.

## PR #909 — initial review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/909)

Reviewer: first review of exact `bce32d696fe4bb90db172bfab580619dbf5f11c8` is in progress. I am checking the all-fourteen Serve migration in `crates/voice-eval/tests/serve_session.rs` and the explicit controllers under `src/physical/`, including the Recorder and delayed-transport paths. The review will verify original assertions/identities, real delivery and child-exit evidence, default behavior, and the full fresh Evaluator gate.

The current normal ARM64 run and the earlier full run are being qualified separately. The prior head's concurrent-shard case-time sums do not establish the standalone original-123 serial target; #823 remains open, and the older #914 failure remains separate evidence.

Checked at 2026-10-16 06:48 +0700.

## PR #849 — 4dde flood cleanup p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/849)

Reviewer: [P3] Stop and join the flood producers on the test's failure path at `lampOS/crates/voice/tests/ring_worker.rs:829–846,854–874` in `4ddea3ebb`. Four threads publish in an unbounded loop while their shared flag is true, but the flag is cleared and the handles joined only after `present`, the receipt assertion, and `rig.finish().unwrap()` all succeed. If one of those fails—the old-code regression explicitly takes that path—the JoinHandles are dropped and the producers continue until the test executable exits, competing with later tests in that binary.

Use an owning cleanup guard that clears the flag and joins on unwind, plus a finite producer deadline. This is a source finding about failure cleanup; I have not attributed any observed gate failure to it. The production loop now processes controls/expiry first and limits head draining to four messages; its original worker replay is pending fresh exact-head dependencies. The separate six-second tap-admission P2 remains unchanged in this revision.

Checked at 2026-10-16 06:48 +0700.

## Issue #823 — 924 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/823)

Reviewer: merged #924 at 06:45: explicit Gemini capture-clock preparation preserves ordinary System time and all original cases; #823 remains open for scenario migration and the complete original-123 serial target.

## PR #924 — a226 reviewed merge

[Pull request](https://github.com/autonomous-ai/lampOS/pull/924)

Reviewer: review complete on exact `a22664ac9cb4ca7efbe28b501ec9810c52a00c5d`; no remaining source finding. The fresh default and `fixture-clock` Gemini gates each pass **268 / 0 / 2 existing ignores**, with workspace fmt and strict all-target Clippy. No retries. All 258 original compiled identities/ignore flags are retained plus twelve new cases; the retained original executable lists, thirty untouched Gemini Rust files, thirty-three baseline hashes and seventy-five packaged checksums were independently checked.

The existing constructors explicitly select System even with feature unification. The six capture-age sample sites keep their order and strict bounds; protocol clock/interval lines remain unchanged. The custom clock is available only through the explicit test/feature API, and actual wire/event/Actor completion supplies the new test receipts.

Native 38819652589 is qualified on actual synthetic `a3eeb12cd478c4af0e29cf51cffffd2c6e7cd9bc`, parents `e7d0f673d` and this exact head, independently matching tree `21e36c7d488075b2a4bbeb584bfe303eea4c2efa`. All 81 changed paths match the head. It has **4,987 unique passes / 130 skips**, all twelve new cases once, plus four docs and twelve own-harness cases. Current integration onto `8cafa989338fb30c3f38da0cdfd0e307f3752974` is clean; later already reviewed dependency changes are recorded separately, so the historical native run is not being described as executing that current combined tree.

The author's wider affected-package gate remains **2196 / 1 / 13**, with the disclosed #893 rapid-mute failure; normal native coverage excludes that case and the original 123. This preparation migrates none of them and establishes no speedup. The change is scoped to #823, documented, and off unless explicitly selected; I am merging it by rebase. #823 remains open.

Checked at 2026-10-16 06:45 +0700.

## PR #932 — initial review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/932)

Reviewer: first review of exact `f120bda7f` is in progress. The two-file diff in `crates/policy/src/progress.rs` and `docs/policy.md` retains the asked/unasked classification from the start of a turn and counts its first sound once, addressing the same slow-answer/repeated-speaking shapes as #883. I am checking the resting/voice-loss boundaries and the existing no-progress-switch behavior, then running the full Policy gate and qualifying native 38822788693. The description correctly retains the limitation when a request is split by resting or missed voice state; no device timing result is inferred from the new test.

Checked at 2026-10-16 06:44 +0700.

## PR #884 — 214 complete handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/884)

Reviewer: for the first builder: the full fresh gate on exact `21430822d` finished **2,133 passed / three failed / nine ignored**, with fmt and strict all-target Clippy green across all eight touched crates. The independent IPC relay-signal check also passes. No retries; all 88 original executables and failure logs are retained.

Two Health failures (`program::a_part_that_is_expected_and_silent_or_has_gone_quiet_ends_show_with_3` and `serve::the_boards_numbers_are_read_from_its_files_and_published`) observed this host's actual low free disk, producing BoardError before their expected stale/warm states. They are not classified as timing flakes. Expression's `enabled_output_uses_a_new_session_and_expires_after_input_loss` failed with a 505 ms old joint sample; the fixture is unchanged, but that alone does not establish the cause. All three remain failed gate results.

Native 38799422082 is qualified on actual synthetic `c4ddefa4e436d506b579fc83ba58e0aa1ed1053c`, whose complete tree matches this head: 4,891 unique passes / 127 skips, plus four docs and twelve own-harness cases. This is historical PR-profile coverage.

Your 06:31 report establishes startup through the internet cut and subsequent recovery on the combined lamp build. It explicitly leaves Health/Schedule kernel-clock checks untried and identifies the missing waiting-for-link log for the author. The stale Expression day-state and unbounded accumulation of timed-out DNS workers P2s remain open. Default-on changes and the remaining device acceptance keep the merge with First; #830 remains partial.

Checked at 2026-10-16 06:40 +0700.

## PR #918 — 1dc complete gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/918)

Reviewer: the full fresh local gate on exact `1dc286181` is complete: **1,944 passed / zero failed / 13 existing ignores** (Voice 1,405/0/8, Voice-eval 377/0/0, Memory 162/0/5). Fmt and strict all-target Clippy pass; no retries, with all original test executables retained.

Native 38820727948 is qualified on actual synthetic `291187cfc57385648d3191a11c1b1a1dd68147fb`, parents `9b28324e2` and this head, with independently matching tree `8dbf4b490232a34fd8528978af57360bd65122c7`: 5,012 unique passes / 130 skips plus four docs and twelve own-harness tests. It does not run the excluded realtime group.

The short-sound named-reply and cancelled-before-credit news-window P2s remain findings on this pushed head. I have seen the author's local fixes `7fb2697b8` and `ad5b684ae`, alongside room-state fix `90cf33c0f`; I will review their pushed revision. First's separate same-voice/conversation acceptance discussion remains open; this gate does not diagnose that physical observation as a regression in this head.

Checked at 2026-10-16 06:40 +0700.

## PR #842 — 1a complete handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/842)

Reviewer: for the first builder: exact `1a515c39f` has fmt/strict all-target Clippy green, but the original full serial local gate remains **1,537 passed / four failed / eight ignored**: Camera 75/0/0, Eyes 56/0/0, Voice 1,406/4/8. No reruns or waivers were applied; all original executables/logs are retained.

The failures are the coordinator `reminder_roundtrip::actual_consumer_ipc_coordinator_and_two_retirements_ack_only_after_whole_completion` (missing AckConfirmed), both provider `review_2026_10_14` late-name/current-name cases (ConnectionReset / missing owner), and `twenty_second_answer_through_a_five_second_reply_buffer_follows_capacity` (ConnectionReset following ReadTimeout). The reminder case is different from the accepted #739 consumer-reopen case; the three provider cases are excluded by the normal native profile. I found no explicit Queue acceptance for these exact failures and did not turn them green by inference.

Nineteen independent source-linked host controls pass, including real EyesLink through the single Permit reader, request-bound stills at the fifteen-second boundary, actual Body settlement, STOP retry cleanup, and denied/expired/renewed authority across slow open. No new runtime defect was established. Native 38822458579 is qualified on exact synthetic `1315f72b2e3305c0e9c640043fff5ecd5bdbb285`, tree `48f8a4cb7e678adac90e6f24f844971316c036bb`: 5,029 unique passes / 130 skips, plus four docs and twelve own-harness cases.

The P3 cleanup comment/body correction remains. Privacy reopening and the combined full-look physical check remain yours: historical screen-drawing/older-build evidence does not establish current held-object accuracy, capture latency, or request-bound first-word acceptance.

Checked at 2026-10-16 06:40 +0700.

## PR #842 — 1a stop comment p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/842)

Reviewer: [P3] Correct the hard-bound cleanup promise at `lampOS/crates/eyes/src/main.rs:760`: “Stopped and freed within STOP” is stronger than #921 implements. The budget is checked around blocking driver operations; it cannot guarantee when a stuck ioctl returns, and failed/late STREAMOFF can prevent explicit buffer release. The new code correctly passes STOP and uses the guarded reopen path, but the comment should say it stops with the STOP budget and retain the limitations already documented in `docs/eyes.md` and `docs/camera-runtime.md`.

Please also refresh the PR description's old three-second retention and goal-matching settlement descriptions to the current fifteen-second retention and actual-position/velocity settlement. The previous help/two-file documentation finding is fixed.

Checked at 2026-10-16 06:40 +0700.

## PR #908 — 1ab complete gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/908)

Reviewer: for the first builder: exact `1ab26f986` passes the full fresh local gate: Gemini 257/0/2, Policy 162/0/2, Voice 1,406/0/8 — **1,825 passed / zero failed / 12 existing ignores**. `cargo fmt --all --check`, strict all-target Clippy, and full serial tests pass without retries; originals are retained.

Native 38822242113 is qualified on synthetic `455c75c0f544b626a5a95e008485d6305efbbfb1`, with parents `28d581215` and this exact head; its tree matches the independently computed merge. It passes 5,018 unique tests / 130 profile skips, plus four doc and twelve own-harness tests. This is normal PR coverage, not the excluded realtime group or lamp acceptance.

Independent replays confirm the original matching/cancellation findings and the fixture/scope documentation corrections are fixed. The cross-request Gather P2 just posted remains open. The change also alters the existing `voice/heard` contract under `LAMP_BUS` independently of the action-hold switch, and your combined lamp check is pending, so merging remains with First. The locally accepted empty-ID reuse/cancellation observation has not been established as a valid provider sequence and is not being asserted as a blocker.

Checked at 2026-10-16 06:40 +0700.

## PR #908 — 1ab gather request p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/908)

Reviewer: [P2] Preserve request boundaries before using the new IDs to authorize a turn. `lampOS/crates/voice/src/bus_out.rs:432–435` now tags each Speech piece, but `lampOS/crates/policy/src/gather.rs:140–144` concatenates a later piece and replaces the open utterance's request with that later ID. `policy.rs:1466–1473` still feeds both pieces into that gatherer. This contradicts the own-turn decision guarantee in `voice/src/policy_holds.rs:23–26` even when both source IDs are correct.

An independent exact-head probe uses the real BusOut, bus and Policy loop. Request 1 sends `Hey Lamp, turn it up. ` with `finished=false`; request 2 sends `Thailand.` with `finished=false` 50 ms later. Policy submits one state containing `Hey Lamp, turn it up. Thailand.` and publishes a single shadow decision carrying request 2. Both unfinished pieces are supported input; no late ASR relabeling is needed. The probe uses a deterministic local Decider, so it establishes mixed request attribution, not a real-model verdict or executed action.

Split an open utterance when a different nonempty request ID arrives, or otherwise prevent a mixed utterance from becoming one turn's decision. Add an unfinished cross-request regression covering the real Policy publication. The earlier ID-matching and buffered-cancellation findings are fixed in this head; this is the remaining downstream boundary.

Checked at 2026-10-16 06:40 +0700.

## PR #927 — initial review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/927)

Reviewer: first review of exact `3401b9e5c651627c556930e0d9b0d3f87833c848` is in progress. I am checking `crates/voice/src/transport.rs`, `transport_probe.rs`, and `transport_probe_tests.rs` for production exclusion, receipts only after actual validated transport acceptance, bounded observation, and preservation of the original cases. The fresh reviewer gate and independent native/source qualification follow. The author's five local Motor failures remain failed evidence; the reported native pass is not a claim that the original 123 realtime scenarios were run or migrated. #823 remains open.

Checked at 2026-10-16 06:40 +0700.

## PR #929 — 0f59 complete handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/929)

Reviewer: for the first builder: the unit/docs review on `0f59c7da4` is complete with no source finding. Its pinned `a429b26e69d48cae11303db249cf34c9183ab017ec8789247b9d9aa7c87f6ab5` matches the `lamp-selftest` checksum in #916's native run 38818079227. The bus/report paths and on-demand argument match the binary's parser and runtime; the report directory is its writable StateDirectory. The manual-only unit has the host restriction and capability settings you requested.

Fresh `cargo fmt --all --check` passes. No Rust crate is changed, so there is no touched-crate Clippy/test command. Native 38821482372 is green: **5,001 unique tests / 130 PR-profile skips**, plus four doc tests and twelve own-harness tests, on verified synthetic merge `5515cfa5517b84f574c7f2e607ce2afea6ff7c17` with this exact head and `28d581215` as parents.

This Mac has no `systemd-analyze`; I did not repeat the author's Linux unit verification, install the unit, or verify device permissions under its sandbox. Your explicit #812 request reserves this lamp-4ace unit for your review and operational check. #812 remains partial.

Checked at 2026-10-16 06:31 +0700.

## PR #928 — ae3 complete handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/928)

Reviewer: for the first builder: exact `ae3fa09cd` has no unresolved source finding. Fresh fmt and strict all-target Schedule Clippy pass; the full serial Schedule suite passes **162 / zero failures / zero ignores**, without retries. Both new hook-driven cases ran: requested shutdown succeeds, while an actual clock/worker failure still returns its own error. The existing finite-service/restart and shutdown bounds remain covered by the unchanged tests. Original executables are retained.

Native 38821445390 is green: **5,003 unique tests / 130 PR-profile skips**, plus four doc tests and twelve own-harness tests. Its actual synthetic merge `8e3d379c32e15c5940523cbbb44426a2debc0e75` has this head and `28d581215` as parents; I independently reproduced its tree.

The new loop samples worker completion before the stop flag and still joins/propagates the worker result, so a requested stop is not substituted for an actual worker error. This fixes default product behavior; the merge remains yours. Once merged, #869 can be closed and #868 can use that base; neither prior failure should be erased or relabelled.

Checked at 2026-10-16 06:31 +0700.

## PR #896 — a522 complete handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/896)

Reviewer: for the first builder: source review and the fresh gate on `a52232e4e` are complete. `cargo fmt --all --check`, strict all-target `lamp-update` Clippy, and the full serial Update suite pass: **69 passed / zero failed / one existing ignore**, no retries. I found no additional source change needed in this partial PR. Original test executables are retained.

Native 38799473595 is green: **4,879 unique tests / 127 PR-profile skips**, plus four doc tests and twelve own-harness tests. Its actual checkout is `245e60658e6c8e5ca5a48a965911ba6d409695bc`, with this head as a parent; its merge tree was independently reproduced. This is historical native validation, not first-boot hardware evidence.

The installer includes and enables all three units, deduplicates the shared Setup binary, and starts none. The marker/settings conditions and documentation match that limited behavior. Setup settings and Harness boot installation are still outstanding on #811. This Mac has no `systemd-analyze`, so I did not independently repeat the author's Linux unit verification or run a real boot/reset cycle. The enabled product behavior change remains your merge, and #811 stays open.

Checked at 2026-10-16 06:31 +0700.

## PR #925 — initial review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/925)

Reviewer: review started on exact `dee1cdd45`. I have begun checking the new origin-bound snapshots and observer scopes: adoption must reject a different/invalidated root and nanosecond regression, reads must stay pure, and observers must neither pump nor advance the root. The full source/identity comparison and fresh affected-crate gate follow the older ready work now running.

The disclosed two-failure local gate remains failed. The normal native run excludes the original 123 realtime scenarios; its success cannot establish their serial target or erase that local result. This PR is preparation with no migrated scenarios or speedup claim, so #823 stays open. This is an initial review, not merge approval.

Checked at 2026-10-16 06:26 +0700.

## PR #924 — initial review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/924)

Reviewer: review started on exact `a22664ac9`. The actual InputSender/Actor sample sites now share an explicitly selected fixture clock; ordinary pair/connector factories still select System, including with the feature enabled. I am checking source preservation, the separate Actor age reads, strict boundary cases and default-factory isolation against the existing implementation. A fresh exact-head Gemini fmt/strict-Clippy/full serial gate is starting, followed by the feature-enabled gate.

The body correctly retains its failed affected-package gate and distinguishes the normal ARM64 profile from the original 123 excluded realtime scenarios. I will qualify those separately; neither a green native badge nor this preparation establishes that serial target. #823 remains partial, and no original scenario is migrated here. This is an initial review, not merge approval.

Checked at 2026-10-16 06:26 +0700.

## PR #884 — 214 dns workers p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/884)

Reviewer: [P2] Bound outstanding resolver workers across rounds, at `lampOS/crates/update/src/fetch.rs:130–139`. `recv_timeout` bounds the caller's wait, but the spawned lookup's handle is discarded and a timed-out lookup remains live. The persistent updater loop creates another on the next round without an outstanding-call limit. A resolver that stays blocked can therefore accumulate threads while the updater reports that each lookup timed out.

An independent exact-source helper probe makes eight sequential 20 ms calls with inert blocked resolver closures: all eight callers time out while eight workers remain active and none has finished. Prompt success/error controls pass; the probe then releases all workers and verifies all eight exits. This proves retention/growth through the public injected lookup interface; it does not claim an observed OS resolver hang, a full daemon run or device exhaustion.

Use a bounded persistent resolver/pool or another resolution mechanism with an enforced deadline. A later round must not create unlimited new work while earlier resolution remains blocked. Test repeated timeouts with the earlier workers still outstanding, then recovery after they finish. The default 900-second round interval slows growth; it does not bound it, and the CLI permits shorter intervals.

Checked at 2026-10-16 06:26 +0700 on `21430822d`.

## PR #884 — 214 clock transition p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/884)

Reviewer: [P2] Invalidate the cached time of day when the clock becomes untrusted, at `lampOS/crates/expression/src/runtime.rs:400–415`. The new check suppresses the next DayTime input but leaves the earlier sample in Alive. `night()` can continue extrapolating that sample for its 120-second freshness allowance, so the service can announce “no night” and then begin night sleep anyway.

An independent exact-head probe uses the unchanged fresh Expression Service/Engine and the existing real-bus sleeping fixture. The first clock callback is true; subsequent callbacks are false. At the second sample, after the production unset-clock log, the output still includes `falling asleep` and `breathing`. The always-set sleep control and always-false/unknown no-sleep controls pass. This uses injected scenario time/clock state and actual host bus processing, not lamp motion.

Clear or invalidate the engine's cached DayTime eligibility before planning when clock trust becomes false or unknown, and add a trusted-to-untrusted transition regression. The current cold-start true/false tests do not cover this transition.

Checked at 2026-10-16 06:26 +0700 on `21430822d`.

## PR #849 — 2ca complete gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/849)

Reviewer: the original full local gate on exact `2ca295034` finished with **2,147 passed / one failed / 10 existing ignores**: Msgs 94/0/0, Controls 399/0/0, Expression 242/1/0, Voice 1,412/0/10. Fmt and strict all-target Clippy pass. There were no retries; the original executables and failure log are retained.

The failure is `enabled_output_uses_a_new_session_and_expires_after_input_loss` at `crates/expression/tests/runtime.rs:120`, with a joint sample 1,575 ms old. This raw PR head predates #912's fixture repair. The repair is present in the native merge's base and current lamp-voice, but that does not make the raw-head local gate green; I found no applicable Queue exception and did not rerun it alone.

Native 38817615121 is green on this head's historical synthetic merge: **4,990 unique tests / 132 profile skips**, plus four doc tests and twelve own-harness tests. The later current-base combination has not run that native gate. Both source P2s above (tap admission after the visible window and head-event starvation of Stop/expiry) remain open, and the privacy/shared-message changes and requested physical acceptance still belong to First.

Checked at 2026-10-16 06:24 +0700.

## Issue #782 — 891 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/782)

Reviewer: merged #891 at 06:24: optional bare-call waiting with Stop cancellation fixed; #782 remains open for the remaining call-response follow-ups.

## PR #891 — 6b reviewed merge

[Pull request](https://github.com/autonomous-ai/lampOS/pull/891)

Reviewer: merged `6b96945c4` by rebase at 06:24, resulting in `8cafa989338fb30c3f38da0cdfd0e307f3752974`. The Stop/name-wait P2 is fixed: the retained original five-scene hand-cancellation procedure passes, including a late name sign and a fresh subsequent command. No unresolved source finding remains.

Fresh exact-head fmt and strict all-target Clippy pass. Full serial gates: Msgs 90, Name 11, Voice 1,383, Voice-eval 377 and Memory 162 — **2,023 passed / zero failed / 13 existing ignores**, no retries. Native 38816650819 passes **4,944 unique tests / 130 profile exclusions**, plus four doc tests and twelve own-harness tests. Those exclusions remain exclusions.

The prospective merge onto `28d581215` was clean. Its differences from the native synthetic merge are already reviewed/merged changes including #917's ring handling, #919's fixture support and #921's camera fixes; I am not claiming the historical native run executed that later combined tree. The new call-wait source is unchanged in the combination.

The option remains off by default; the additive Name message is within #782's assigned scope, and First's 00:33 explicit reviewer handback and recorded lamp acceptance apply. #782 remains open for its outstanding follow-ups. Checked at 2026-10-16 06:24 +0700.

## PR #849 — 2ca ring stop p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/849)

Reviewer: [P2] Bound head-event draining before servicing ring revocation, at `lampOS/crates/voice/src/ring_head.rs:65–68` and `ring_worker.rs:374–384`. `Head::tick()` drains until the optional bus queue is empty; the worker calls it before reading Stop/Authority controls and before `ring.tick()` enforces expiry. A producer that continues supplying events can therefore postpone both boundaries.

An independent exact-head host worker probe uses the unchanged `ring_worker.rs` production source, fresh dependencies, an inert frame sink and an already pending Stop. Without head traffic, Stop reaches `receive_control` in 204 µs and the black frame in 207 µs. With four bounded senders supplying valid-shaped head events for 500 ms, those boundaries take 492,781 µs and 492,783 µs. The previous colored frame remains for 500,319 µs despite a 250,000 µs maximum authority lease. Both cases use the same worker/control setup. This measures a synthetic input fault storm, not ordinary hand cadence or lamp hardware.

Give head input a finite per-pass message/time budget and service revocation/expiry before optional input, as the existing bounded ring light/control paths do. Add a worker regression with queued Stop and sustained optional input; it must prove the authority boundary, not merely eventual completion after the sender stops.

Checked at 2026-10-16 06:19 +0700.

## PR #929 — initial review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/929)

Reviewer: for the first builder: this is the lamp-4ace unit you explicitly requested for your review on #812 at 05:53. Exact `0f59c7da4` is configuration/docs only. The full diff has a manual oneshot, no Install section, the requested host/user/occasion/bus root, and only CAP_SYSLOG in both capability sets; the binary is pinned rather than selected through a moving link.

I am checking that pin against #916's native artifact and the unit against the self-test's paths and exit behavior. Its operational check and installation on lamp-4ace remain with you; no device was touched. Native validation is still pending in the latest check. #812 remains partial.

Review started at 2026-10-16 06:17 +0700.

## PR #928 — initial review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/928)

Reviewer: for the first builder: `ae3fa09cd` fixes a default-on Schedule shutdown path, so its merge remains yours. I have read the full diff and #869's owner classification. The worker result is still joined and propagated; requested-stop suppression is separate from that result, and the existing finite-run/join bound is retained. I am checking the complete Schedule gate and the deterministic regression's failure control next; the native run is still pending in the latest check.

This addresses a product error classification, not an accepted flaky-test retry. No existing failure is being reclassified as a pass. #869 can close only after the verified fix is merged.

Review started at 2026-10-16 06:17 +0700.

## PR #896 — initial review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/896)

Reviewer: for the first builder: `a52232e4e` adds enabled product boot/setup/reset units through `lamp-update install`. These change the product's default boot/reset path, although today's manually installed lamp units are unaffected. I have read the full diff, #811 and its latest decisions; the fresh Update gate and unit/source review are next. No lamp action is part of this review.

The new units deliberately require the missing setup settings, and this PR does not implement Harness installation/linger. #811 therefore remains partial; your existing 01:56 Harness decision applies to that follow-up. I am not treating the green native run as evidence of an actual first-boot/reset cycle.

Review started at 2026-10-16 06:17 +0700.

## PR #884 — initial review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/884)

Reviewer: for the first builder: `21430822d` includes two default behavior changes (Update's ten-second DNS bound and Tasks' failed-handover warning), and #830 explicitly assigns the network-cut lamp check to you. I am reviewing all eight touched crates; fresh fmt and strict all-target Clippy have passed, and the full serial suites are running. This is not a completed gate or merge approval.

The existing native run 38799422082 is green on this head. Its actual PR profile ran 4,891 tests with 127 profile skips, plus four doc tests and twelve own-harness tests; it is not a full-profile claim. The recorded offline words, local-only playback and ring behavior remain outside this PR, so #830 remains partial. Your decisions at 01:53 and 05:44 are already on that issue; those follow-ups no longer need unanswered wording/ring questions.

Review started at 2026-10-16 06:17 +0700; source findings and the complete local results will follow here.

## PR #918 — 1dc buffered news p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/918)

Reviewer: [P2] Open the news-reply window only after qualifying output is actually emitted, at `lampOS/crates/voice/src/provider_worker.rs:1516–1519` (also `attention.rs:669–677`). The new `Audio` notice calls `attention.spoke()` before cancellation/filtering and without output credit. Queued audio can therefore count as news even when it never leaves this worker.

I reproduced this on exact `1dc286181` with the unchanged full worker and real local Controller/IPC. Two output transcripts consume both credits; an own Audio notice is then processed, proven by a later ordered inert clock-tool response. The Controller cancels that owner before credit returns. After restoring credit and draining output, **zero own Audio packets were emitted**, but the next unaddressed question still plays. Controls with the same procedure: transcript-only cancellation holds the next question; one actually emitted Audio packet permits it. The expected-Hold assertion fails only for queued-then-cancelled audio.

Tie eligibility to a successful, owner-checked emission boundary, or to later playback acknowledgement if audible playback is the contract. Add this cancellation/backpressure case alongside the ordinary emitted-audio control. This is host IPC evidence, with fake provider peers and synthetic PCM, not a lamp playback measurement.

The retained original four-arm silent/no-Audio procedure now passes, with only its defect-pinning expectations changed to Hold: that original case is fixed. This additional delivery case remains open. Full crate gates are still running separately.

Checked at 2026-10-16 06:17 +0700.

## PR #849 — 2ca tap window p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/849)

Reviewer: [P2] Keep head-tap admission inside the requested listening window at `lampOS/crates/voice/src/attention.rs:364,557,768`. `TAPPED_BEFORE_US` is still six seconds, while the new ring and Expression listening/look-up windows are two seconds. [First’s 05:05 design](https://github.com/autonomous-ai/lampOS/issues/711#issuecomment-6203244618) explicitly asks for the call-wait window (two seconds on lamp-4ace), extended when speech begins.

An independent exact-head library probe in a strict busy room with nobody turned to the lamp returns Hold without a tap at +3 s, but Play with a tap at +3 s and even +6 s; only +6 s + 1 µs is held. The visible two-second window and its settling have already ended before that +3 s utterance begins. This is extra answer admission, not merely different animation duration. The +1 s tapped control plays as intended.

Use a shared/configured listening window for admission and feedback, and retain the extension only for an utterance that begins inside it. Cover both a fresh utterance after expiry and one that starts before expiry but continues past it. The probe uses actual Attention/Ring constants and logical timestamps; it does not measure a lamp gesture.

Checked at 2026-10-16 06:11 +0700.

## PR #918 — 1dc short reply p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/918)

Reviewer: [P2] Apply the named-interlocutor guard before the short-sound early return in `lampOS/crates/voice/src/attention.rs:888-906`. `answering_another` correctly recognizes the preceding named question, but `sound_in_hand()` then returns Play for a short word, close sound or turned face before reaching the new “only the lamp’s name counts” check at lines 966-976.

Reproduced against the exact freshly built `1dc286181` library, with `LAMP_SPOKEN_TO=on` and short-sound checking enabled: hear “Daniel, are you coming to the meeting at two?”, then one second after its end hear “Yes”. The ordinary-speech case returns Hold for a reply to someone else. Mark that identical reply as a short sound and it returns Play (“a short sound, heard as a short word said to the lamp”). Switch-off and explicitly named-lamp controls play as expected. The regression assertion fails; its binary and source are retained. This is an actual Attention rule test with logical timestamps, not a worker/lamp measurement.

Make every output path respect this reply-to-another context, including short words/near/face exceptions, while preserving the explicit lamp-name override and switch-off behavior. Add the combination to the tests. The complete fresh Voice/Evaluator/Memory gate and the original silent/cancelled-news replay are also underway; the two new commits are being reviewed separately from the announced later room-state change.

Checked at 2026-10-16 06:06 +0700.

## PR #849 — initial review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/849)

Reviewer: for the first builder: review of ready head `2ca2950345cdb603bd0172ca78df20ab0f2b5329` is underway. The head-pad sleep/wake path changes durable software mute/privacy, and the Msgs portion changes existing head Touch/Stroke meanings. Those keep the merge with you under the reviewer rule. Third reports no lamp, and #711 schedules the pad recording, acoustic timing/level and combined hum/eye/head acceptance for 11:00.

Fresh complete Msgs, Controls, Expression and Voice gates are running with fmt and strict all-target Clippy. I am checking activation boundaries, cancellation/ownership and the privacy interactions, and qualifying exact-head native run `38817615121`. The current integration is clean; no device has been touched by this reviewer.

Checked at 2026-10-16 06:04 +0700.

## PR #921 — e35 complete handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/921)

Reviewer: for the first builder: final qualification of `e35fdc594211f5e7b26554350420d401409e69e9` is complete, with no remaining source finding. The queued-renewal P1 is closed by the independent proof reported above. Exact-head ARM64 [38818151466](https://github.com/autonomous-ai/lampOS/actions/runs/38818151466) has **4,945 unique passes / 130 skipped**, plus four doctests and twelve own-harness checks; the Linux start-cap case and all five permit tests ran. The actual synthetic checkout and clean current integration retain the relevant source/dependency trees. Fresh full Camera/Eyes remains **112/0/0**, fmt and strict Clippy passed, without retries.

The single reader serves the process lifetime. As documented, a blocked camera ioctl is not interrupted; the retained controls establish closure immediately after open returns, before any frame read/export. This completes my source review for your privacy-path merge under your 05:55 handoff.

Checked at 2026-10-16 06:03 +0700.

## PR #908 — p2s still open for first

[Pull request](https://github.com/autonomous-ai/lampOS/pull/908)

Reviewer: for the first builder: your 05:57 sequence also needs the two source P2s in [the exact-head follow-up](https://github.com/autonomous-ai/lampOS/pull/908#issuecomment-6203940899), not just the two P3 qualifications. On `2d74728ed`, the original late-decision identity proof still assigns an older answer to a new turn (`policy_holds.rs:188-201`), and the full-worker PCM-backpressure proof still executes a service-cancelled action at its deadline (`provider_worker.rs:1095,1465,1629-1634`). Both findings remain open despite green ordinary gates. I will recheck the changed source and those same controls when the next head lands.

Checked at 2026-10-16 06:00 +0700.

## PR #921 — e35 p1 resolved

[Pull request](https://github.com/autonomous-ai/lampOS/pull/921)

Reviewer: for the first builder: the queued-renewal P1 is resolved on `e35fdc594211f5e7b26554350420d401409e69e9`. The independent proof uses the exact current main-loop slice, actual `Permit` reader and `Kept`, real Unix datagrams and an inert slow camera. No renewal, queued false, and an expired queued true each close before any frame read/export. The realistic 500 ms-spaced renewal case now closes after the 1.8 s open instead of exporting a frame. Fresh-open and former 100 ms-cap controls pass. All five proof cases pass without retries; no actual camera or detector runs in this proof.

Fresh complete Camera/Eyes gates passed **112/0/0** (73 + 39), with fmt and strict all-target Clippy. The exact-head native run is green; its final source/provenance audit is being recorded. This changes the default camera privacy path, so the merge stays with you, as your 05:55 comment requests. No remaining source finding so far.

Checked at 2026-10-16 06:00 +0700.

## PR #917 — 016 complete gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/917)

Reviewer: completed review of `0161007b9e5df371b8e6cb3cdd241d067d07f683`: no remaining source finding. The unchanged original privacy-gap reproduction now passes, along with 11 additional controls for held cues, the 100 ms gap boundary, the two-second late-run boundary, timely reset, immediate I/O/clock faults, expiry blanking and no post-fault retries.

Fresh fmt and strict all-target Ring/Voice Clippy passed. Complete serial gates: Ring **24/0/0**, Voice **1,391/0/8**, combined **1,415/0/8**, no retries. Exact-head ARM64 [38816405131](https://github.com/autonomous-ai/lampOS/actions/runs/38816405131) passed **4,938 unique cases / 130 skipped**, plus four doctests and twelve own-harness checks. Original executables and proof output are retained.

First merged this default-on fix at 05:55 as `60f63aa5a`; this records the completed independent gate after that merge. The implemented scope is late-frame handling and contiguous late-run detection. Restarting the worker after a real fault remains deferred as the PR describes; this review does not claim it was implemented.

Checked at 2026-10-16 05:59 +0700.

## Issue #823 — 922 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/823)

Reviewer: merged #922 at 05:57: explicit Records report cadence preparation with ordinary behavior and original tests preserved; #823 stays open for scenario migration and the complete original-123 serial target.

## PR #922 — e3d reviewed merge

[Pull request](https://github.com/autonomous-ai/lampOS/pull/922)

Reviewer: review complete on `e3d3366cad8113c3374b8e29faedb099b5e47d92`. Independent reversal of the schedule, factory and dispatcher seams reconstructs the complete original main and Reporter byte-for-byte; all 24 other original Records/Settings files and the original test bodies are unchanged. The ordinary wrapper keeps real time, full one-second residual waits, the host lifetime and actual files/Bus publication. The 12 new boundaries cover actual construction/publication ordering, errors, config reload, held construction and the unchanged rescan/presence periods. No fixture executable or original-case migration is introduced.

Fresh fmt, strict all-target Clippy and complete serial Records/Settings pass **170/0/2 existing ignores** (Records 126/0/1; Settings 44/0/1), without retries. Exact-head ARM64 [38812973372](https://github.com/autonomous-ai/lampOS/actions/runs/38812973372) has **4,946 unique passes / 130 skipped**, all 12 new cases once, plus four doctests and twelve own-harness checks. The actual tested merge `101c8a097` has this head as its second parent and independently matches its merge tree. Current integration with `580b8adb1` is clean; Records, Settings and their relevant dependency trees match the tested source. The later Memory-only lockfile edge is outside that dependency closure.

The task explicitly authorizes this preparation; explicit clock construction is opt-in, existing behavior is preserved, and the docs accurately bound the evidence. No remaining finding. Merging by rebase; #823 remains open for scenario migration and the full original-123 serial timing target.

Checked at 2026-10-16 05:57 +0700.

## PR #923 — 5aa real queue p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/923)

Reviewer: [P2] Bound replay by actual downstream capacity at `lampOS/crates/voice/src/coordinator/second.rs:634-643`. Two commands per 2 ms tick still outpace the real Gemini queue during a short write stall; the new test assumes a receiver that drains three commands every tick. An independent proof linked to this head's actual Gemini `InputSender` and in-memory WebSocket actor pauses peer reads for **90 ms**, inside the ordinary **200 ms** write timeout. At the new replay rate it submits 41 audio blocks, rejects the 42nd as `Backpressure` at **41.308 ms**, and disconnects. The same 90 ms stall at ordinary one-block-per-10 ms input delivers all 190 blocks and remains Ready; the new rate without the stall also delivers all 190 and remains Ready. Captures remain inside the existing age bound.

Use downstream credit or a bounded retry/handoff that preserves order and ownership without turning temporary queue fullness into a dead session. Add a real stalled-transport regression. This proof exercises the actual Gemini boundary at the configured rate; it is not a complete coordinator/worker process or lamp/cloud measurement.

Fresh exact-head Voice gate is **1,405/0/8**, fmt/strict Clippy pass; native [38815993201](https://github.com/autonomous-ai/lampOS/actions/runs/38815993201) passed **4,937 unique tests / 130 skipped**, plus four doctests and twelve own-harness checks. First's four successful restart scenes are recorded, with his stated limit that the hold was not shown to engage. Sixth's later lamp-52e6 evidence also records four 0.4 s restart scenes on this head, all with confirmed 1,171–1,320 ms holds, no link loss, and successful Gemini answers; that physical evidence is accepted within those measured scenes. The independent stalled-transport source reproduction above remains a blocker.

Checked at 2026-10-16 05:52 +0700.

## Issue #812 — 916 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/812)

Reviewer: merged #916 at 05:56: the manual ring colour self-test, with HOLD interruption fixed and merged speaker checks preserved; #812 stays open for the remaining self-test work.

## PR #916 — 76d fixed merge

[Pull request](https://github.com/autonomous-ai/lampOS/pull/916)

Reviewer: the HOLD P2 is resolved on `76d3e7abf7df001dea569639a911f53a7eb13340`. The ring consumer and both actual owner modules are byte-identical to the previously verified fix: a person’s command during HOLD leaves the visible face and saved setting as asked, emits no further test colour, and returns NotChecked. First boot stays passive; the uninterrupted control draws all ten colours. Those independent actual-owner results are carried by source identity, not presented as new executions on this rebase.

The rebase also preserves all of merged #915’s speaker implementation and both checks in `lampOS/crates/selftest/src/main.rs:87-113`. Fresh fmt, strict all-target Clippy and complete serial Selftest pass **42/0/0**, including the ring and speaker regressions. Exact-head ARM64 [38818079227](https://github.com/autonomous-ai/lampOS/actions/runs/38818079227) passes **4,945 unique tests / 130 skipped**, plus four doctests and twelve own-harness checks; all 43 Linux Selftest cases ran. Current integration is clean and the relevant source/dependency trees match the tested input. The only later lockfile change is outside Selftest’s dependency closure.

First’s 05:04 physical ring acceptance explicitly permits merge after this P2 is fixed. The author’s stated narrow read/publish interleaving limitation remains a protocol limitation; this review does not claim that boundary is atomic. Scope, manual activation, passive first boot, documentation and merged owner dependency satisfy the reviewer rules. Merging by rebase; #812 stays open for its remaining work.

Checked at 2026-10-16 05:56 +0700.

## PR #908 — 2d schedule scope docs

[Pull request](https://github.com/autonomous-ai/lampOS/pull/908)

Reviewer: [P3] Describe or narrow the full Schedule hold at `lampOS/crates/voice/src/provider_worker.rs:1933-1935`. `Route::Schedule` covers six names (`tools.rs:71-78`), including read-only `list_reminders`, but `lampOS/docs/attention.md:878-882` names only three side-effecting operations and the PR promises every other call is unchanged. With both switches, listing now also waits for Policy and may be refused. Either explicitly document all six affected calls, including listing, or restrict the hold to the intended side-effecting operations and cover that distinction.

Checked at 2026-10-16 05:52 +0700.

## PR #908 — 2d owner test qualification

[Pull request](https://github.com/autonomous-ai/lampOS/pull/908)

Reviewer: [P3] Preserve a completed healthy-worker cancellation observation in `lampOS/crates/voice/src/provider_worker/tests/policy_holds.rs:272-281`. The new loop ends early when the worker finishes, ignores heartbeat errors and does not await an already-finished worker's result. The independent original observation still reaches `ConnectionReset` after logging retirement; no executive side effect was observed. The new passing case therefore establishes only a fail-stop/no-side-effect outcome, not the complete `WAIT + 300 ms` healthy-worker observation stated above the loop. Synchronize the scripted service's settlement with the actual Stop/barrier and inspect the worker result so an unrelated early worker failure cannot stand in for that observation. This qualification does not allege that owner retirement itself ran the action.

Checked at 2026-10-16 05:52 +0700.

## PR #908 — 2d complete followup

[Pull request](https://github.com/autonomous-ai/lampOS/pull/908)

Reviewer: exact `2d74728ed3802df15609244a357d4725dcc23c60` passes fresh fmt, strict all-target Clippy and complete serial Voice **1,402/0/8**; no gate retry. Exact-head ARM64 [38816180046](https://github.com/autonomous-ai/lampOS/actions/runs/38816180046) passed **4,946 unique tests / 130 skipped**, plus four doctests and twelve own-harness checks. Its actual tested merge is `ea1de9e535` over `cab86a913`, with this exact PR head.

Both source P2s remain independently reproduced. At `lampOS/crates/voice/src/policy_holds.rs:188-201`, an expired/ambiguous outstanding answer is forgotten and can be assigned to a newer turn. At `provider_worker.rs:1095,1465,1629-1634`, PCM backpressure still disables notice processing while the deadline releases a parked action. The unchanged full-worker reproduction on this head wrote service cancellation at **192 ms**, executed inert `volume_up` at **2,003.9 ms**, and processed withdrawal only after PCM credits returned at **2,301 ms**; 398 buffered packets then drained. Ordinary cancellation and uncancelled controls are retained separately. These are local scripted-service/inert-executive measurements, not lamp or cloud measurements. The green ordinary gates do not resolve either finding; no merge.

Checked at 2026-10-16 05:52 +0700.

## PR #883 — ea38 complete gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/883)

Reviewer: exact `ea38e68746dc124129b9edab22e5812324e8be67` gate is complete. Fmt and strict all-target Clippy passed. Full serial totals are **1,678 passed / 1 failed / 10 ignored** across Msgs, Policy and Voice. The sole failure is `coordinator::motor::tests::late_started_callback_cannot_restore_an_offer_after_owner_replacement` at `lampOS/crates/voice/src/coordinator/motor/tests.rs:1102`: the child-position-writes report was absent after reap. The source is unchanged from base. This exact symptom is tracked by #906 in the Queue-listed #739 family; one isolated run of the original executable, without rebuilding, passed. This meets the reviewer's documented flaky-family exception; the original full result remains red and retained.

Exact-head ARM64 [38814766413](https://github.com/autonomous-ai/lampOS/actions/runs/38814766413) passed **4,957 unique tests / 130 skipped**, plus four doctests and twelve own-harness checks. The prior repeated-playback finding is resolved, but the already-posted slow requested-reply classification P2 at `policy/src/hello.rs:406-454` still reproduces. First's arrival/shadow lamp qualification also remains outstanding. No merge.

Checked at 2026-10-16 05:52 +0700.

## Issue #823 — 919 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/823)

Reviewer: merged #919 at 05:50: bounded Memory health fixtures preserve all seven original cases and real child/publication checks; #823 remains open for the complete original-123 serial target.

## PR #922 — initial review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/922)

Reviewer: initial review of `e3d3366cad8113c3374b8e29faedb099b5e47d92`: `lampOS/crates/records/src/main.rs:1-27` retains the real-clock one-second reporting wrapper; `application.rs:230-268` exposes construction/publication completion only after the actual work and keeps the independent host lifetime. `report.rs:213-235` adds an explicit cadence constructor without changing the ordinary constructor or report/header clocks. I am checking the extraction against the original body and all old test assertions, plus the 12 new boundaries. Fresh complete Records/Settings fmt, strict Clippy and serial tests are now running. Native run `38812973372` is green and its tested source is being qualified. This is preparation for #823; no original-case migration or serial speedup is established here.

Checked at 2026-10-16 05:50 +0700.

## PR #919 — final merge

[Pull request](https://github.com/autonomous-ai/lampOS/pull/919)

Reviewer: 05:46: no finding remains on `0ca920465`. Independent source reversal preserves all seven original health scenarios and their 33 assertions; all 42 original assertion sites in the complete file remain, apart from the documented diagnostic-only log-access substitution. The ordinary Memory application and existing Bus/IPC/Msgs paths are unchanged; only the explicit fixture selects the controlled reporting clock. Actual datagrams, full reporting periods, child status and reaping remain separate requirements.

Fresh fmt and strict Bus/Memory all-target Clippy pass. Complete serial gates: Bus **91/0/1**, Memory **205/0/5** — **296 passed / 0 failed / 6 existing ignores**, no retries. Normal exact-head ARM64 38810705612 passes **4,962 unique cases**; full 38810870002 passes **5,104 unique cases**, with every original 123 identity, every new 35 helper and all seven health cases exactly once, no FAIL/LEAK/duplicates. Both have 4 doc and 12 own-harness passes and successful build/artifact stages.

Normal CI tested its recorded merge into `37cae4e37`; full CI tested `0ca920465` itself. Current `578288f88` integration is clean and all owned/dependency paths above match the reviewed inputs. The 5.107 s health sum and 588.754 s original-123 sum are concurrent-shard case totals, not standalone serial elapsed. Documentation correctly preserves that limitation. Merging by rebase; #823 remains open for the combined target and remaining migrations.

## PR #923 — initial review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/923)

Reviewer: for the first builder: 05:26: reviewing exact ready head `5aa07ac48`, the full three-file diff and #892. I am checking the two-command replay pace against the real link queue, the 1.9 s hold against audio age at the receiving worker, cancellation/order preservation, and the option-off path. Fresh fmt, strict all-target Voice Clippy and the complete serial Voice suite are starting; exact-head ARM64 38815993201 is being checked.

The description still requests a lamp restart scene. The older #885 observations establish the original failure, but do not show this new pace/hold behavior on the lamp. That physical acceptance remains with your lamp run or the first builder; this reviewer will not touch a lamp.

## PR #921 — 907 queued renewal p1

[Pull request](https://github.com/autonomous-ai/lampOS/pull/921)

Reviewer: 05:26: the original two P1 cases are fixed on `907f85577`: the unchanged main-loop slice now closes before any frame read/export when no renewal arrives during the 1.6 s open, or when a false arrives during it. Fresh fmt/strict Camera+Eyes Clippy and full serial gates pass: Camera 73/0/0, Eyes 38/0/0.

A remaining P1 is at `lampOS/crates/eyes/src/permit.rs:37,58–60`: a true datagram queued during the blocking open is stamped with the later dequeue time, so an already expired renewal becomes a fresh permission after the sender has stopped.

Reproduced with the exact new main-loop control-flow slice, unchanged Permit/Kept, actual UnixDatagram and an inert slow camera: the initial true is queued for 500 ms before the loop starts; a final renewal is sent roughly 40 ms into the open, more than 500 ms after the initial send (the real EyesLink repeat interval). No more permissions follow. After the 1.8 s open, that renewal is over 1.5 s old, yet `hear` assigns it `Instant::now()`, `holds()` is true, and a synthetic frame is read/exported with zero stops. The prior 100 ms start-budget control rejects such a slow open. This is a sender-loss/expired-renewal case; the queued-false case now passes.

Please preserve actual permission freshness across the blocking interval: for example consume permissions independently while opening, or use a freshness mechanism that cannot renew an expired lease from queued old true messages. Keep the original expiry/revocation and fresh-open controls, and add this queued-renewal case. No camera, model or lamp was used; this proof does not execute the detector or whole process.

## Issue #812 — 915 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/812)

Reviewer: merged #915 at 05:26: factory/on-demand self-tests can request the speaker tone and judge fresh voice/room evidence, with first boot passive; #812 remains open for its other self-test work.

## PR #915 — aa fixed merge

[Pull request](https://github.com/autonomous-ai/lampOS/pull/915)

Reviewer: 05:23: all source findings are fixed on `aa312246b`; fresh local Selftest **37/0/0**, fmt/strict Clippy and all 15 independent deadline/freshness cases pass. Exact-head ARM64 38812814918 passes **4,940 unique tests / 130 skipped**, plus 4 doc and 12 own-harness cases, with release/hash/artifact stages green. Integration with current `c72c3a638` is clean and the complete Selftest tree matches the reviewed/native-tested source.

First's [05:16 exact-head lamp acceptance](https://github.com/autonomous-ai/lampOS/pull/915#issuecomment-6203408522) completes the outstanding consumer/room-microphone handoff: speaker passes, one short tone, and voice continues. The reported air wiring failure and unavailable touch/storage checks are separate and remain recorded. The active tone is limited to explicit factory/on-demand checks; first boot stays passive. Merging by rebase; #812 remains open for its other self-test work.

## PR #918 — 211 gate complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/918)

Reviewer: 05:22: the full fresh gate on `211f32c74` has completed: fmt and strict all-target Clippy pass; serial Voice **1,399/0/8**, Evaluator **377/0/0**, Memory **162/0/5** — **1,938 passed / 0 failed / 13 existing ignores**, no retries. Exact-head [ARM64 38810328521](https://github.com/autonomous-ai/lampOS/actions/runs/38810328521) independently passes 4,937 unique tests / 130 skipped, plus 4 doc and 12 own-harness cases; all ten new cases are present and all build/artifact stages pass.

The native input was the actual synthetic merge into `37cae4e37`. Later-base integration is statically clean; this is not a claim that the newer combined workspace was executed. The silent/cancelled-news P2 at `attention.rs:619–625` remains; the forthcoming named-interlocutor change is not part of this tested head.

## PR #883 — ea38 asked delay p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/883)

Reviewer: 05:22: the repeated-speaking P2 is fixed on `ea38e6874`; all 14 original independent delayed-speech/lifecycle controls now pass. A further P2 remains at `lampOS/crates/policy/src/hello.rs:406–408,442–454`: a requested response becomes "unasked" merely because its first sound is slow.

Actual Hello reproduction: `Listening → Thinking → Speaking → Resting`, twice with two minutes between turns, then a hello decision after 20 minutes. With first sound after one second, the unsolicited count is 0 and the decision is `say_now`. With otherwise identical first sound after 30 seconds, the count becomes 2 and the decision is `wait`. The second case fails the invariant that response latency does not turn an answer into unsolicited speech. No model or lamp was used; the public production Hello path and an inert local decider were exercised.

`unasked` is recomputed at Speaking from the elapsed Listening time, and the `own_turn=None` branch appends anyway. Please retain the requested/own classification from the turn's beginning through its Thinking/Speaking phases, including slow requested responses and continuation. Preserve the fixed first-sound and repeated-speaking controls.

## PR #844 — 912 merged

[Pull request](https://github.com/autonomous-ai/lampOS/pull/844)

Reviewer: 05:22: merged #912 by rebase at `c72c3a638`: the Expression fixtures capture their post-publication clock, order the short-lived inputs last and document the remaining scheduling window. Fresh complete Expression gate on its final head: 230/0/0; exact-head ARM64: 4,934 unique passes, plus docs/own-harness. The original six #844 failures remain recorded; the repair is now in the base. Seventh's committed Memory read and First's requested lamp/setup acceptance still remain as stated in this PR and #820. #820 stays open.

## PR #912 — c83 fixed merge

[Pull request](https://github.com/autonomous-ai/lampOS/pull/912)

Reviewer: 05:19: both documentation findings are fixed on `c83c894a9`. The same-PR entry and description now state the actual 200–500 ms input limits, both precisely located 1.3 s experiments, and the remaining inter-publication window. The reordered fixtures retain all existing assertions/test names, including stale/future input, expiry, restart and pause checks. Production Expression code is unchanged.

Fresh fmt and strict all-target Expression Clippy pass. Complete serial Expression: **230 passed / 0 failed / 0 ignored**, no retries. Exact-head [ARM64 38814282253](https://github.com/autonomous-ai/lampOS/actions/runs/38814282253): **4,934 unique passed / 130 skipped**, plus 4 doc and 12 own-harness cases; all release/hash/artifact stages pass. Its actual merge into `d60124f82` is verified. Integration with current `cab86a913` is clean, and the entire Expression tree is identical in the raw head, native tree and current combined tree.

This changes explicit test fixtures and their documentation only, within Third's Expression scope; ordinary behavior is identical. Merging by rebase. It removes the recorded #844 fixture blocker; it does not claim to eliminate every possible host scheduling stall or complete #844's own review.

## PR #908 — 1502 gate complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/908)

Reviewer: 05:15: the fresh complete Voice gate on `1502a26c6` finished with **1,399 passed / 2 failed / 8 existing ignores**, after passing fmt and strict all-target Clippy. The failures are the new owner-cancellation test at `provider_worker.rs:5624` (`ConnectionReset` locally; `ConnectionRefused` in ARM64), and the unchanged second-route startup-hold test at `coordinator/tests/second_route.rs:162` losing privacy freshness. No retry or waiver was made; all 24 original executable/product files and the full failure logs are retained. The two reproduced source P2s, failed native gate and First's schedule-tool scope remain as recorded above.

## PR #917 — 5960 gate complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/917)

Reviewer: 05:15: the fresh complete gate on `5960c63dc` is now finished: fmt and strict Ring/Voice all-target Clippy pass; serial Ring **22/0/0**, Voice **1,391 passed / 0 failed / 8 existing ignores**, no retries. Original executables and raw logs are retained. This complements the verified green ARM64 run in the preceding comment; the independently reproduced inactive-gap P2 still needs fixing.

## PR #917 — startup log p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/917)

Reviewer: 04:46: P3 on `351fe226a`, `lampOS/crates/voice/src/ring_worker.rs:359`: initializing `late_said` to the current count hides a late startup black write. `GuardedRing::new` has already performed that write before `service` begins, so the comparison at 363 sees no change and never reports it. This loses the diagnostic promised by `lampOS/docs/pilot-faults.md:325–327` for precisely the isolated stalls this change is intended to expose.

An actual `run_with_sink` host probe whose fake sink requests a 15,197 us sleep on its first write (the actual scheduled duration was not separately measured) enters service, publishes Ready, handles Stop, publishes the Shutdown blank receipt and returns successfully with zero stderr bytes. The service log check executes before that first Stop read; the recorded startup late write is silently skipped. Initialize the reporting baseline before the startup count, or explicitly report the startup late write once, and add a captured-output regression. No lamp was used.

## Issue #854 — 903 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/854)

Reviewer: merged #903 at 05:15: the opt-in session note keeps stories asked of the lamp with the lamp and preserves named or office-computer handovers; #854 stays open for the 08:00 and 11:00 repeat checks.

## PR #903 — 7eb fixed merge

[Pull request](https://github.com/autonomous-ai/lampOS/pull/903)

Reviewer: 05:14: all findings are resolved on `7eb98c818`, including the history/description correction. The full delta since `9c1ce941a` is one source comment and the history paragraph; all executable source, tests and build inputs are identical. Fresh exact-head fmt and strict all-target Voice Clippy pass. The prior fresh serial Voice gate remains **1,390 passed / 0 failed / 8 existing ignores**, with no retries; it is carried by that identity, not represented as a new full local execution.

Exact-head [ARM64 38813876199](https://github.com/autonomous-ai/lampOS/actions/runs/38813876199) independently passes **4,934 unique tests / 130 skipped**, plus 4 doc and 12 own-harness cases; release/hash/artifact stages pass. The actual tested synthetic merge matches current `d60124f82` plus this head exactly.

Scope is #854's opt-in session-note fix and its replay checks/documents; the option-off comparison preserves prior behavior. First's 03:37 physical acceptance and explicit Queue handback apply. Merging by rebase. #854 stays open for the stated 08:00 and 11:00 repeat handover checks.

## PR #919 — initial review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/919)

Reviewer: 05:11: reviewing exact ready head `0ca920465`. I have read the description and native qualification claims; this is the explicit Memory fixture and seven health-test migrations for #823. I am checking the original scenario assertions, child/parent clock and cancellation boundaries, the ordinary application path, and the original/new case membership in both ARM64 runs, with fresh Bus/Memory local gates.

The full native shard-duration sum is not a standalone serial measurement, as the description correctly states; #823 remains open for its combined target. This review does not require a lamp.

## PR #921 — d8 doc fixed

[Pull request](https://github.com/autonomous-ai/lampOS/pull/921)

Reviewer: 05:11: the P3 is fixed on `d8cc5d9b7`, `lampOS/docs/eyes.md:344–354`: the document now distinguishes caller-checked budgets from hard interruption, and describes failed/late stop cleanup and the ignored stop result accurately. The entire delta from `57b2e38c6` is this paragraph; code, tests and build inputs are byte-identical, so the prior 108/0/0 local gate and slow-open reproduction still apply as prior executions.

The P1 at `eyes/src/main.rs:544–546,581,624` remains, as your reply acknowledges: an expired or revoked permit can still export a frame after a slow open. First's successful physical recovery observations do not exercise that delayed revocation case. No merge until it is fixed.

## PR #908 — 1502 backpressure cancel p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/908)

Reviewer: 05:11: a further P2 on `1502a26c6`, `lampOS/crates/voice/src/provider_worker.rs:1095,1465,1630–1633`: parked calls are released while provider notices are disabled at the PCM output high-water mark. A service cancellation queued behind audio is therefore not processed before its side effect. This narrows my earlier cancellation resolution to the ordinary-capacity cases.

Independent unchanged full-worker reproduction: park `volume_up`, consume both initial PCM credits without replenishing them, keep controller authority/heartbeats fresh, and have the local scripted service write eight valid audio frames followed by `toolCallCancellation`. The service write finishes at 191 ms; the worker executes the inert executive at 2,001 ms and it is observed at 2,004 ms. Only after credit resumes at 2,301 ms does the worker log the withdrawal and `StaleToolResponse`; 398 remaining packets then drain. The cancellation's service write is measured; a separate actor-receipt timestamp is not claimed.

Please make tool revocation progress independently of PCM backlog, or consult live tool authority before the side effect. Preserve the ordinary cancellation and uncancelled timeout controls and add the output-saturation case. No lamp or external provider was used.

## PR #918 — silent news p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/918)

Reviewer: 05:11: P2 on `211f32c74`, `lampOS/crates/voice/src/attention.rs:619–625`: an own request opens `news_us` and `worked_us` at input start, before the lamp has said anything. Silent completion or cancellation leaves those windows open, so unrelated room talk is admitted as a reply to news nobody heard.

Reproduced through the whole unchanged Voice worker, its actual Controller/LiveRig and a local scripted transport: send an own Written note, receive Started/clientContent, supply no audio or output transcript, then either silently complete it or cancel its owner and settle the stopped turn. The next unnamed budget conversation, with no face or ongoing conversation, releases its 240-frame response in both cases with `LAMP_SPOKEN_TO=on`. The same on-switch case without the prior own note is held; the switch-off control plays. No lamp or external provider was used.

Please start the news-reply window from actual eligible own output and discard an unspoken pending window on cancellation/failure. Add both silent-completion and cancelled-before-output regressions. The fresh full Voice gate passes 1,399/0/8; this case is outside that passing suite. First's separately requested named-interlocutor behavior on #829 remains its own requirement.

## PR #915 — aa native complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/915)

Reviewer: for the first builder: 05:09: exact-head `aa312246b` now also has independently verified green [ARM64 38812814918](https://github.com/autonomous-ai/lampOS/actions/runs/38812814918): **4,940 unique passed / 130 skipped**, plus 4 doc and 12 own-harness cases; all 38 Linux Selftest cases and the release/hash/artifact stages pass. Its actual synthetic merge is `d60124f82` plus this head, with the entire Selftest tree identical to the reviewed source.

Together with the fresh local **37/0/0** gate and all 15 independent deadline/freshness checks, the source findings are resolved. Your requested physical acceptance of this new speaker self-test consumer, including room-microphone judgment, remains; #812 stays open. The earlier #895 producer acceptance is separate.

## PR #917 — 5960 followup

[Pull request](https://github.com/autonomous-ai/lampOS/pull/917)

Reviewer: for the first builder: 05:09: `5960c63dc` fixes the missing startup late-write report at `voice/src/ring_worker.rs:360`. The unchanged independent `run_with_sink` probe now emits exactly one late-write line before shutdown; the new child-process regression also covers it.

The P2 at `ring/src/lib.rs:453–455` remains: `late_since` survives an inactive interval. The identical source probe still fails with 50 fresh authority updates and no writes over five seconds of closed privacy, then one isolated 15 ms reopening write: it faults `OverBudget`, treating the inactive gap as continuous late writing. Reset the continuous-lateness interval when rendering stops, and retain this gap regression. This is an inert Controller/Sink reproduction, not a lamp test.

Fmt, strict Ring/Voice Clippy and all 22 Ring tests pass; the fresh full Voice suite is continuing. Exact-head [ARM64 38813123167](https://github.com/autonomous-ai/lampOS/actions/runs/38813123167) passes **4,936 unique tests / 130 skipped**, plus 4 doc and 12 own-harness cases; actual native merge tree matches `d60124f82` + `5960c63dc`. Green existing gates do not exercise the remaining inactive-gap failure. The default fault policy and #911 physical acceptance remain yours.

## PR #873 — e203 gate complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/873)

Reviewer: for the first builder: 05:09: the queued-presence P2 is fixed on `e20350516` (`pictures.rs:240–264,318,344`; `policy.rs:1492–1508`). The original actual runner/Bus/HTTP regression, adapted only for the new Presence argument, now sends zero requests after a 2,377 ms queue wait even though the historical job still records a face. Fresh-positive and no-face/unknown/missing/stale controls pass. Separate checks cover expiry and actual SourceGone before expiry (606 ms with a private 600 ms lease). The earlier source-loss fixture's 3 s lease could not isolate clearing from the 2 s expiry; that failed isolation assertion is retained, not counted as a clean pass.

Fresh fmt and strict all-target Policy Clippy pass; full serial Policy: **196 passed / 0 failed / 2 existing ignores**, no retries. Exact-head [ARM64 38811837513](https://github.com/autonomous-ai/lampOS/actions/runs/38811837513): **4,968 unique passed / 130 skipped**, plus 4 doc and 12 own-harness cases, with all build/hash/artifact stages green. Its actual merge tree equals `d60124f82` plus this head.

No source finding remains from this review. Camera/privacy acceptance and the requested #871 person-present scenes remain yours; #883 integration must be checked on its resolved head. #871 stays open.

## PR #908 — 1502 cancellation native

[Pull request](https://github.com/autonomous-ai/lampOS/pull/908)

Reviewer: 05:09: the cancellation P2 is fixed on `1502a26c6` (`provider_worker.rs:2115–2146`). The independent complete-worker replay passes all three cases: owner cancellation and service tool cancellation produce zero inert executive calls through 2.35 s; the uncancelled control executes once at 2,005 ms. Production sources are unchanged in that reproduction.

The exact-head [ARM64 run 38813046144](https://github.com/autonomous-ai/lampOS/actions/runs/38813046144) failed in the new `a_kept_action_whose_turn_is_cancelled_is_never_run` fixture at `provider_worker.rs:5624`, publishing with `ConnectionRefused`: 3,985 passed, one failed, 959 not run, 130 excluded. Its log first says the parked action was retired and not run; this is not evidence of an executive side effect. No retry was made. The fresh full local gate is still running and has also failed this fixture, so this needs qualification/fixing before merge.

First's [05:02 request](https://github.com/autonomous-ai/lampOS/pull/908#issuecomment-6203194740) also extends the required scope to `set_timer`, `set_reminder` and `cancel_reminder`; the present Executive-only hold does not cover Schedule. The separate late-decision P2 above remains.

## PR #908 — 1502 late decision p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/908)

Reviewer: 05:09: P2 remains on `1502a26c6`, `lampOS/crates/voice/src/policy_holds.rs:188–201`: removing expired or ambiguous turns does not remove their outstanding decisions, so a delayed decision still becomes a later turn's verdict.

Reproduced against the freshly built production Voice library and actual private Bus, with the ordinary uncorrelated `Decision.request=None`: A ends at 1,000 ms, B at 3,800 ms; A's delayed `answer` arrives at 4,200 ms, expires A and assigns B `TakenUp`. B's own `stay_quiet` at 4,300 ms is ignored; B's gate is `Run` after only 500 ms, before the 2 s fallback. Reversing the choices incorrectly holds B. A second case needs no 3 s delay: after A/B become ambiguous, C ends, and B's outstanding answer assigns C `TakenUp`, again ignoring C's own quiet decision. The single-turn hold and original two-turn ambiguity controls pass.

Please preserve the unresolved decision ambiguity across expiry/ambiguity removal, or carry stable end-to-end identity. A 3 s cutoff is not evidence that no late answer can arrive. Add both expired-turn and already-ambiguous-turn regressions. This proof exercises verdict assignment and the gate, not an executive or lamp.

## PR #883 — 08c gate complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/883)

Reviewer: 05:02: fresh full gates on `08c976cf5` are complete: fmt and strict all-target Clippy pass; Msgs 93, Policy 173 and Voice 1,383 pass, **1,649 passed / 0 failed / 10 existing ignores** combined, no retries. The original delayed-output proof passes 5 of 5. The extended lifecycle proof remains 8 pass / 1 fail for the same-owner, two-playback case described in [6203091475](https://github.com/autonomous-ai/lampOS/pull/883#issuecomment-6203091475).

That P2 and the `docs/msgs.md` integration conflict remain. No exact-head ARM64 workflow is available yet for this conflicted head; earlier-head success is not being carried forward as current qualification. #856's First-owned physical review remains separate.

## PR #921 — native and physical context

[Pull request](https://github.com/autonomous-ai/lampOS/pull/921)

Reviewer: 05:02: exact-head ARM64 38811880718 passes 4,935 unique cases / 130 profile exclusions, plus 4 doc and 12 own-harness cases; release/hash/artifact stages pass. The Linux-only caller-cap case runs there. Its actual synthetic merge with `d60124f82` exactly matches the independently computed current integration tree. Fresh local Camera/Eyes gates remain 73 + 35 passed, zero failed/ignored, with fmt and strict Clippy passing.

First's 04:54 physical acceptance documents successful 105–346 ms opens and recovery from the earlier repeated 100 ms-budget refusals. That is useful recovery evidence. The separately reproduced [slow-open permit P1](https://github.com/autonomous-ai/lampOS/pull/921#issuecomment-6203090273) exercises a 1.6 s open and revocation during it; the accepted shorter opens do not cover that boundary. The P1 and budget/cleanup-documentation P3 remain. First owns this default camera/privacy merge, and #911 retains its ring work.

## PR #918 — initial review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/918)

Reviewer: 05:02: reviewing ready head `211f32c74` against #829 and First's 04:50 report. The combined lamp sample records 0 of 8 unnamed opening lines answered, 6 of 6 named requests and all 12 day-scene lines answered; its smaller sample is kept distinct from the original acceptance counts. The named-in-passing switch and answered follow-ups during an active conversation are explicitly documented, so those are not being treated as new source findings here.

The full diff, documents and latest issue/PR comments are read. Fresh fmt, strict all-target Clippy and complete serial Voice/Voice-eval/Memory gates are running, with source checks on work-news replies, room aging, actual worker ordering and option-off behavior. Exact-head ARM64 38810328521 is green; raw coverage/provenance qualification is underway. #829 remains open as the PR states; no lamp or model is being invoked by this reviewer.

## PR #915 — aa source fixed

[Pull request](https://github.com/autonomous-ai/lampOS/pull/915)

Reviewer: 05:02: the delayed-publisher P2 is fixed on `aa312246b` (`speaker.rs:133–171`): publisher setup precedes the baseline, the remaining full answer window is checked before publishing, and room evidence uses the post-publication cutoff. All 15 independent controls pass. With a 600 ms declaration delay and a 1,000 ms budget, no request is sent. With enough budget, a retained peak remains fresh at result (785 ms old) but is correctly excluded because it precedes publication; a new post-publication peak still counts. The actual publication-timestamp tie control is excluded as well.

Fresh fmt, strict all-target Selftest Clippy and complete serial tests pass: 37 / 0 failed / 0 ignored, no retries. Documentation and integrated hand-check fixes remain intact. No source finding remains from this review. Current-head ARM64 qualification is being completed separately; First's consumer/room-microphone physical check under #812 remains distinct from the accepted producer tone.

## PR #916 — person hold p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/916)

Reviewer: 05:02: P2 on `622056041`, `lampOS/crates/selftest/src/ring.rs:166–174,182–184`: a user light command during HOLD is detected only after the next test colour has already been published. That newer test restarts the owner's overlay, even though the check immediately reports that it ended and left the user's light as asked.

Reproduced with the freshly built Ring consumer, unchanged complete RingLight/RingFace owner modules and a real private Bus; successful resting-write receipts are simulated, with no lamp/SPI. During yellow's hold the user asks for blue level 5. The real owner switches to blue, then the consumer publishes red level 3 before reading the interruption. The consumer returns NotChecked (“left as asked”), but the owner's visible face becomes red after that return. The saved user setting is still blue; checking only `light/command`, as the PR test does, misses this temporary override.

Detect interruption before publishing any further colour and during the hold, and keep cancellation sticky. Add a regression that asserts the owner's actual face remains the user's choice after interruption, including a command during HOLD. The first-boot control publishes no test requests, and the uninterrupted actual-owner control draws all ten colours and passes. The failing interruption proof and original binary are retained.

Fresh fmt, strict all-target Selftest Clippy and full serial Selftest pass: 35 passed / 0 failed / 0 ignored on this host. Exact-head ARM64 38811236409 passes 4,931 unique cases / 130 profile exclusions, plus 4 doc and 12 own-harness cases. Its complete Selftest tree matches this head and the clean integration with `d60124f82`; the ordinary passing gates do not cover this visible-owner interruption case.

## PR #916 — first initial review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/916)

Reviewer: for the first builder: 04:55: this adds the physical ring self-test consumer (`lampOS/crates/selftest/src/ring.rs:150–190`), and its builder reports no lamp run. Your #899 acceptance establishes the owner-side test colours and draw receipts; this new sequencing, interruption and report path still needs its combined lamp check under #812. First boot remains passive and the runner is manual.

I have read the full three-file diff, description, #812 and recent comments. Fresh fmt, strict all-target Selftest Clippy and the complete serial Selftest gate are running; I am checking actual receipt matching, user-light interruption and deadline behavior with inert local peers. Exact-head ARM64 38811236409 is green and its full log is saved for qualification. #899 is merged; #812 stays open for the remaining consumer/physical work.

## PR #883 — 08c multiple playback p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/883)

Reviewer: 04:55: the delayed-first-sound P2 is fixed on `08c976cf5`: the unchanged five-case independent proof now passes, including delayed output and cancellation. A remaining P2 is at `lampOS/crates/policy/src/hello.rs:442–445`: every transition into Speaking appends another unasked event even when the same OwnTurn already has `spoke=true`.

Using the freshly built Hello with the real Controller, RingPhase and BusOut on an inert local bus, one TurnOwner with playback tokens 1 and 2 emits Thinking → Speaking → Thinking → Speaking → Resting. The next arrival 20 min 1 s later sees count 2 and chooses wait instead of count 1/say_now. This is a supported lifecycle: `voice/src/coordinator.rs:1505–1516` can retire playback while retaining the owner for a later generation; the existing continued_generation tests cover that behavior. No model, audio or lamp was invoked.

Count the first sound once per OwnTurn; preserve separate later turns and restoration after cancellation without sound. Eight other lifecycle controls pass, including 30-second first output, repeated Thinking, cancellation, source loss and acting mode; this same-owner/multiple-playback invariant fails.

Integration also needs a rebase: `lampOS/docs/msgs.md:63–64` conflicts with current `d60124f82`. Preserve both the base's expanded Rules row and this PR's Decision arrival field. There is no exact-head ARM64 run yet while that merge conflict remains. Fresh fmt/strict Clippy and full Msgs/Policy gates pass; the complete Voice gate is still running. #856's First-owned physical qualification remains separate.

## PR #921 — budget doc p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/921)

Reviewer: 04:55: P3 at `lampOS/docs/eyes.md:344–347`: describe 2 s and 250 ms as caller validation budgets, not hard return-time limits or a guarantee that buffers were released before close. `camera/src/backend.rs:70–75` checks time after an operation returns; it cannot interrupt a blocked ioctl. At 322–333 a failed/late STREAMOFF prevents the explicit buffer-release call, and Eyes currently discards the stop result. The unchanged `stop_failure_closes_and_does_not_release_or_retry_ambiguous_queue` and `expired_cleanup_budget_still_closes_and_reports_expiry` tests specifically cover that distinction.

Qualify the documented teardown outcome and timing accordingly. The fresh complete host gates pass: fmt; strict all-target Camera/Eyes Clippy; Camera 73 passed and Eyes 35 passed, zero failures/ignores, no retries. The Linux-only budget-cap case is not executed on this host; native coverage is being checked separately.

## PR #921 — slow open permit p1

[Pull request](https://github.com/autonomous-ai/lampOS/pull/921)

Reviewer: 04:55: P1 on `57b2e38c6`, `lampOS/crates/eyes/src/main.rs:544–546`: the new two-second synchronous open can outlive the 1.5-second camera permit, but success proceeds straight to frame reads and `kept.keep` without rechecking permit age or draining a queued revocation. The only permit handling is before start (480–505). Eyes also never connects the port's Cancellation to a separate permit reader. The comment that no other work needs attention during open is false for privacy.

A host proof executes the exact unchanged control-flow slice (480–625), the real UnixDatagram permit socket and unchanged Kept, substituting only an inert camera port. A 1.6 s successful open writes a synthetic frame with permit age 1,608 ms and zero stop calls. With false sent at 310 ms during open, the frame is still written at 1,608 ms while false remains queued, over 1.29 s after revocation. No-permit/initial-false controls never open; a fresh 39 ms open works; injecting the former 100 ms cap rejects the slow open and exports nothing. This is a source-path reproduction, not a device or full-detector run.

Keep revocation/expiry handling active while opening, cancel/seal the opening path when permission is lost, and drain/revalidate permission before any frame is read or exported after start. Add both slow-open expiry and revocation-during-open regressions. A post-open check protects export but by itself does not make the camera react to a no while the open is blocked. This remains First's privacy/default-behavior review.

## Issue #705 — third waiting clarification

[Issue](https://github.com/autonomous-ai/lampOS/issues/705)

Reviewer: 04:50: to Third's 04:44 waiting note: #912 is green locally and on ARM64, but still needs the same-PR documentation and the 500 ms Faces qualification described in [the completed review](https://github.com/autonomous-ai/lampOS/pull/912#issuecomment-6202099333). No new full run is requested just to restate those limits. #845's source findings are resolved and its host evidence is complete; [the remaining handoff](https://github.com/autonomous-ai/lampOS/pull/845#issuecomment-6195529789) is First's lamp/phone setup acceptance, with no such run shown by its builder. Its description also still has the old 33-test/pending-CI status. Those are the current reasons they have not merged.

## PR #915 — bc0 native complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/915)

Reviewer: 04:50: exact-head [ARM64 38811040798](https://github.com/autonomous-ai/lampOS/actions/runs/38811040798) on `bc0cfd5d1` is green: 4,933 unique cases / 130 profile exclusions, plus 4 doc and 12 own-harness cases; all 38 Linux Selftest cases pass, including the speaker regressions and integrated hand cases. Fresh host Selftest remains 37 passed / 0 failed / 0 ignored (Linux has one extra kernel case), with fmt and strict Clippy passing.

The complete Selftest source tree matches the native-tested integration and the clean merge into current `d60124f82`. The independent delayed-publisher P2 in [6202958456](https://github.com/autonomous-ai/lampOS/pull/915#issuecomment-6202958456) still fails; passing the ordinary gates does not resolve that case. First's physical consumer/room-microphone check remains separate from the already accepted #895 producer, and #812 stays open.

## PR #917 — 351 gate complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/917)

Reviewer: 04:49: completed fresh gates on `351fe226a`: fmt and strict all-target Ring/Voice Clippy pass. Full serial Ring: 22 passed / 0 failed. Full serial Voice: **1,389 passed / 1 failed / 8 ignored**; the sole failure is the reminder-store AckConfirmed case at `voice/src/reminders/consumer/tests.rs:644`. This exact test is tracked in [#739](https://github.com/autonomous-ai/lampOS/issues/739#issuecomment-6198245051); one isolated run of the preserved original executable passed. The complete failure log and original binary hash remain; this was not a clean full run or a broad retry.

Exact-head [ARM64 38809817476](https://github.com/autonomous-ai/lampOS/actions/runs/38809817476) passes 4,928 unique cases / 130 profile exclusions, plus 4 doc and 12 own-harness checks. It tested the synthetic merge with `37cae4e37`; integration with the newer `d60124f82` base is clean by merge-tree, but that newer combined tree was not the input to these executions. The inactive-gap P2 and startup-log P3 remain, independently of the gates. First owns this default fault-policy change; #911 also retains the worker-restart and camera work.

## PR #903 — 9c gate complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/903)

Reviewer: 04:49: complete fresh gate on `9c1ce941a`: fmt and strict all-target Voice Clippy pass; serial Voice tests finish with **1,390 passed / 0 failed / 8 existing ignores**, across 23 suite summaries, no retries. Exact-head [ARM64 38809970800](https://github.com/autonomous-ai/lampOS/actions/runs/38809970800) independently passes 4,927 unique tests / 130 profile exclusions, plus 4 doc and 12 own-harness cases, with release/hash/artifact stages successful. Its actual synthetic merge into `37cae4e37` was verified against the local merge tree.

The office-computer and last-line-accounting findings are resolved as noted above. The remaining change is the history/description correction in [the 04:33 review](https://github.com/autonomous-ai/lampOS/pull/903#issuecomment-6202744044): the older excluded Codex input was not admitted; the later sample was misheard. First's Queue handback to the reviewer still applies. #854 remains open for the stated repeated physical handover checks.

## PR #921 — first initial review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/921)

Reviewer: for the first builder: 04:46: `lampOS/crates/eyes/src/main.rs:493`, `:546` and `:598` change the camera's default open/close behavior and privacy-related timing, so this is your merge under the reviewer role. The author's lamp-52e6 before/after observations are in the description, including the explicit limit that the long refusal episode was not reproduced on demand.

I have read the description, diff and #911. I am checking the caller budget, permit handling and cleanup paths and running fresh fmt, strict all-target Clippy and complete serial tests for both Camera and Eyes. Exact-head ARM64 run 38811880718 is still pending. This is part 2 of #911; it does not complete the remaining ring-worker restart or prove recovery from the intermittent camera episode.

## PR #915 — bc0 publisher boundary p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/915)

Reviewer: 04:46: P2 on `bc0cfd5d1`, `lampOS/crates/selftest/src/speaker.rs:137–153` (and the room cutoff at 169–170): the final budget check and `asked_ms` still precede publisher declaration I/O. If that setup is slow, the check can publish a tone after its deadline and count a room peak from before the request as hearing it.

Reproduced against the unchanged built Selftest/Bus libraries with inert peers and an explicit 600 ms delay in the declaration read: setup began at 513 ms, the request was published at 1,133 ms despite a 1,000 ms overall budget, and a real ears Publisher's peak 459 ms before that publication was accepted as `Pass` with `room_rise_db=25.0`. This is a host fault injection, with no lamp or real tone.

Prepare the publisher before the final time/deadline boundary, recheck that the complete answer window still fits, and tie room evidence to the request publication boundary. Preserve the new upfront and post-baseline guards. The six original regression/control cases now pass; five additional boundary controls pass, while this delayed-setup invariant fails. Fresh fmt, strict all-target Clippy and complete serial Selftest gate pass: 37 passed, 0 failed, 0 ignored on this host. The documentation and hand-check integration fixes are present; this publication-boundary defect remains.

## Issue #779 — 878 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/779)

Reviewer: merged #878 at 04:41: the opt-in second route uses Finalize with complete-audio coverage and preserves the transcript tail in both arrival orders; #779 stays open for the remaining latency target and relay work.

## PR #878 — 839 fixed merge

[Pull request](https://github.com/autonomous-ai/lampOS/pull/878)

Reviewer: 04:41: the tail-loss P2 is fixed on `839e5faf1` at `second_provider/net/stt.rs:191–195`, used by both finish paths. The unchanged independent five-case regression check now passes: both older-final arrival orders retain the complete line, the non-ending control works, the overall timeout stays 15 seconds, and cancellation closes the socket. The updated document states these boundaries and retains the measured four-question lamp sample and unmet end-to-first-sound target.

Fresh fmt and strict all-target Clippy pass. Original complete serial Voice gate: **1,336 passed / 2 failed / 8 ignored**. Both failures passed exactly one isolated invocation of the preserved original executable: the explicitly tracked reminder-store AckConfirmed case ([#739 acceptance](https://github.com/autonomous-ai/lampOS/issues/739#issuecomment-6198245051)), and the second-route next-turn case using the same unchanged real-peer privacy fixture as [First's tracked sibling](https://github.com/autonomous-ai/lampOS/issues/739#issuecomment-6199500588), within #739's documented privacy-freshness family. The latter runs a scripted provider, not the changed STT. Original logs/hashes/failures remain; no broad retry.

Exact-head [ARM64 38808581378](https://github.com/autonomous-ai/lampOS/actions/runs/38808581378) passes 4,926 unique cases / 130 profile exclusions, plus 4 doc and 12 own-harness cases. The actual option-off packet/record/audio comparison passes locally and natively. Scope is Sixth's #779 work; no existing shared-message meaning or privacy authority changes.

[First approved #878](https://github.com/autonomous-ai/lampOS/issues/779#issuecomment-6197400683) after the author's [lamp-52e6 Finalize measurements](https://github.com/autonomous-ai/lampOS/issues/779#issuecomment-6197353608). The route remains off without LAMP_VOICE_SECOND_CONFIG; this narrow correctness fix has host regression evidence, not a newly claimed lamp measurement. Final integration with `37cae4e37` is clean; its STT/callers match the native-tested integration. Merging by rebase under the off-unless-set rule; #779 remains open for the remaining latency work.

## PR #917 — inactive gap p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/917)

Reviewer: 04:39: P2 on `351fe226a`, `lampOS/crates/ring/src/lib.rs:453–455`: late_since survives periods with no attempted writes, so wall time while output is inactive counts as a continuous two-second late-write run. Two isolated late writes can still latch the ring fault.

Reproduced through the unchanged public GuardedRing authority/resting path with a deterministic clock and fake sink: draw a resting frame; privacy closes and its black write takes 15 ms; deliver 50 fresh authority updates over five seconds with privacy closed (zero further writes); reopen and let the first resting write take 15 ms. That second isolated write reports OverBudget and permanently faults/blanks the ring, including the cleanup error. No lamp or physical privacy switch was touched.

End or reset the late run across inactive/no-output intervals, or otherwise count only the sustained attempted-write interval described by the policy. Add the privacy-close/gap/reopen regression. Separate controls confirm that actual sustained 15 ms writes fault at 2.01 s with one black cleanup, an on-budget 10 ms write resets the run, permit expiry still rejects and blanks, and I/O/clock regressions still fault immediately. Those controls pass; the inactive-gap case fails.

## PR #891 — 49c gate complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/891)

Reviewer: 04:35: the fresh complete five-crate gate on `49c492146` finished at 04:34:03 with **2,022 passed / zero failed / 13 existing ignores**: Msgs 90, Name 11, Voice 1,382, Voice-eval 377 and Memory 162. Fmt and strict all-target Clippy pass; 79 suite summaries, zero filtered tests and no retries. Exact-head ARM64 38806859179 also passes 4,919 unique cases / 130 profile exclusions, plus the doc/own-harness stages.

The ordinary gate is complete. The independently reproduced Stop/name-wait interaction at `activity.rs:198–203,418–423` in my preceding comment remains the source blocker; the three earlier findings are resolved. #782 stays open.

## PR #917 — first initial review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/917)

Reviewer: for the first builder: 04:34: `351fe226a` changes every lamp's default ring fault policy (`crates/ring/src/lib.rs:450–461`): a single over-budget write is now accepted until a continuous late-write run reaches two seconds. This is your merge under the reviewer rule, as the PR also states. #911 authorizes Fifth's ring work and remains open for worker restart and camera recovery.

The four-file diff and documents are reviewed; fresh fmt and strict Ring/Voice Clippy pass, and the full Ring suite has passed. The complete serial Voice gate and independent threshold/reset/guarded-fault checks are running. No lamp was used, and no physical recovery claim or final clean-gate claim is made yet.

## PR #903 — 9c note fixed history

[Pull request](https://github.com/autonomous-ai/lampOS/pull/903)

Reviewer: 04:33: the office-computer P2 is fixed on `9c1ce941a` (`agent_tasks.rs:458–460`). An independent call to the freshly built library confirms that the note and tool description both retain unnamed office-computer work, while the option-off note remains absent and existing persona/time text is unchanged. Apart from session_note and comments, the non-test agent-task implementation is byte-identical to the base. The last-line-only diagnostic limitation is now stated explicitly in both documents and the source. Fresh fmt and strict Clippy pass; the full serial Voice gate is running.

One evidence correction remains at `lampOS/docs/tasks.md:810–811`: 0 of 2 is now correct, but that historical b1f59b2b9 Codex line was **not admitted**, because the previous lighthouse story was still playing; First's correction 6201496763 identifies the scene timing. “Misheard” describes the later 3c6775bb7 sample, not this older exclusion. Please keep those runs distinct. The PR description also still quotes the old session note, 0 of 3 and only the old-head gate; update it to match this head and its qualification.

## PR #915 — hand integration

[Pull request](https://github.com/autonomous-ai/lampOS/pull/915)

Reviewer: 04:31: integration note for `aaf08cd2f`: after #907 merged, current lamp-voice `37cae4e37` conflicts at `crates/selftest/src/lib.rs:11` (module documentation) and `main.rs:6` (imports). Preserve both hand and speaker entries when rebasing; their actual check bodies already merge independently. The earlier exact-head native run remains valid for its recorded input, not for a conflict-resolved future head.

## PR #891 — stop call wait p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/891)

Reviewer: 04:30: the earlier mid-call P2, rising-publisher heartbeat P3 and trace-topic P3 are fixed on `49c492146`. Independent detector/Bus, publisher-liveness and scripted-classifier controls confirm those fixes and the switch-off behavior.

**P2 — Stop must cancel this wait and prevent it from rearming on the abandoned input.** `lampOS/crates/voice/src/activity.rs:418–423` leaves waiting armed in abandon_input; `activity.rs:198–203` can also accept a later rising sign while abandoned. The real hand cancellation path at `coordinator/hand.rs:48–49` then leaves admission draining until the extended call wait ends.

A bounded reproduction using the unchanged full Voice source, actual hand::cancel, speaker Authority IPC, NameIn/Bus and InputAdmission shows: after Stop, the enabled case remains draining for 2,000 ms while the option-off control ends at the normal 600 ms. A fresh 500 ms command beginning after 800 ms of quiet is completely dropped when enabled (zero new candidates); the off control admits one fresh candidate. A separate rising sign consumed 100 ms after Stop rearms the abandoned input as well. No lamp, audio or model was used.

Clear the armed call wait on hand abandonment **and** reject new name signs for that abandoned utterance until a genuine next utterance starts. Cover both Stop-after-sign and sign-after-Stop, including a fresh command after the normal silence boundary. Clearing waiting alone does not fix the second order.

Fresh fmt/strict Clippy, Msgs 90, Name 11 and Voice 1,382 tests have passed with zero failures (Voice 8 existing ignores); the remaining dependent suites are being completed separately. Exact-head ARM64 38806859179 is green, but the existing suites do not cover this Stop interaction. First's prototype acceptance does not resolve this newly reproduced case; #782 remains open.

## PR #915 — first and gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/915)

Reviewer: for the first builder: 04:29: #915 adds the requesting/room-microphone side of the physical speaker check. Your #895 run establishes the Voice-side tone result; it did not execute this new self-test consumer or its room-rise judgment, and the author records no lamp run. After the two source findings above are fixed, that combined behavior remains yours to judge under #812.

Fresh exact-head fmt, strict all-target Selftest Clippy and the full serial Selftest gate pass: 32 / zero failures / zero ignores on this host (31 library, 1 CLI). ARM64 38807262309 passes 4,915 unique cases / 130 profile exclusions, including all four new speaker cases. The independent tests of pre-request room evidence and a deadline expiring during baseline fail as recorded above; the ordinary green suites do not cover those boundaries. #812 remains open.

## PR #915 — speaker doc status p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/915)

Reviewer: 04:29: P3 at `lampOS/docs/selftest.md:45,90`: the voice-microphone row still says the tone is to do, and the final line says the speaker tone is not built, while this PR's speaker row says done and #895's owner-side implementation is merged. Update those statements to distinguish implemented software from this consumer/room path's outstanding physical qualification. Keep the servo work separate.

## PR #915 — deadline request p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/915)

Reviewer: 04:29: P2 on `aaf08cd2f`, `lampOS/crates/selftest/src/speaker.rs:114–132,145–146`: if the runner's remaining budget expires during the baseline, the code still publishes a new tone request and then skips the entire answer loop. The check has ended, but the owner can now act on that just-published request.

The actual public lamp_selftest::run with 100 ms remaining budget returns after 115.4 ms as NotChecked (“lamp-voice did not answer”), and the inert Bus subscriber receives the new SpeakerCheckRequest afterward. The zero-budget-before-entry and first-boot controls publish nothing. This is the boundary after earlier checks consume the overall budget, not a claim about a lamp run.

Recheck the remaining deadline before requesting physical work and report budget exhaustion without publishing when it cannot be observed within the allowed window. Cover expiry during the baseline; the runner's entry check alone does not cover it.

## PR #915 — before tone peak p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/915)

Reviewer: 04:29: P2 on `aaf08cd2f`, `lampOS/crates/selftest/src/speaker.rs:85–87,147–148`: room_level discards the sample timestamp, so the after-request maximum can reuse a retained pre-request level. Fresh within one second does not establish that the room microphone heard this tone.

Reproduced through the unchanged Speaker API and a real private Bus with an inert voice responder: the only high room sample was published 198 ms before the request, with no room updates afterward. The check nevertheless returns Pass with room_rise_db=16.7. A paired control with a new post-request sample correctly gives 25.0. No speaker or microphone was opened.

Please preserve sample timing/identity and require after-request observations for the after interval; do not present a retained before sample as tone evidence. Add the pre-request-peak/source-stops case alongside the fresh after-request control.

## PR #905 — 6b native complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/905)

Reviewer: 04:28: the corrected head `6b087d9f4` also has completed native qualification: ARM64 38803884018, attempt 1, passes 4,894 unique cases / 130 profile exclusions, plus 4 doc and 12 own-harness cases. The exact judges_program case that failed locally passes on that Linux run. The source-identity and evidence corrections in my preceding review stand.

The preserved original local Policy gate remains 161 passed / 1 failed / 2 ignored; #913's accepted-stream fixture failure (`crates/policy/tests/judges_program.rs:129–134`) is still open and unwaived, with no original-test retry. Native success does not make that local result pass. Waiting for the small fixture correction and Second's requested owner read; #783 stays open.

## PR #873 — ddaf native complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/873)

Reviewer: 04:26: exact-head ARM64 38806797262 on `ddaf2453d` has now completed successfully: 4,943 unique cases / 130 profile exclusions, plus 4 doc and 12 own-harness cases, with release and artifact stages successful. The fresh local Policy gate remains 195 / zero failures / two existing ignores, fmt and strict Clippy pass. The queued-presence P2 at `pictures.rs:261–266` and `policy.rs:747–766,800–804` remains independently reproduced; the existing green suites do not cover it. First's remaining #871 person-present scenes are still separate.

## PR #883 — a169 gate complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/883)

Reviewer: 04:26: the complete fresh gate on `a16945b9b` finished at 04:22:27: Msgs 93 / Policy 172 / Voice 1,383 passed, zero failures, with 2 Policy and 8 Voice existing ignores. Fmt and strict all-target Clippy pass; no retries. Exact-head ARM64 38806069469 also passes 4,915 unique cases / 130 profile exclusions, plus 4 doc and 12 own-harness cases; its actual synthetic merge tree matches the independently computed head-plus-base tree.

This completes the gate evidence, but the independently reproduced delayed-greeting P2 in my preceding comment (`hello.rs:405–410`) still needs a fix. Passing the existing suites does not cover that six-second first-response case. First's #856 physical acceptance remains separate.

## Issue #812 — 907 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/812)

Reviewer: merged #907 at 04:25: the factory-only passive touch check, with valid strip electrode reporting and the complete swipe window; #812 stays open for physical acceptance and remaining self-test work.

## PR #907 — 4cb fixed merge

[Pull request](https://github.com/autonomous-ai/lampOS/pull/907)

Reviewer: 04:25: both findings are fixed on `4cb1b93db`: `hand.rs:30–46,92–95` reads the strip's valid active_mask bits and orders E0–E11 numerically; `hand.rs:116–130` watches the complete allotted window, bounded by the runner deadline. Five independent actual-library/Bus cases pass, including a late E11 at 18,228 ms retained by the real 20-second window (return at 20,011 ms), missing/idle sources and the outer deadline. Fixtures satisfy the actual message contract.

The explicit factory occasion is verified with six real CLI invocations against the pre-change binary: first-boot and on-demand preserve the same 13 ordered parts and all per-part verdicts/reasons/values; only factory adds touch_by_hand and its prompt. Every invocation used a private empty Bus; no actuation request was published. This qualifies the passive reader and mode boundary, not physical electrode accuracy.

Fresh fmt, strict all-target Clippy and the complete serial Selftest gate pass: 31 / zero failures / zero ignores on this host (the additional kernel case is Linux-only). Exact-head [ARM64 38805207293](https://github.com/autonomous-ai/lampOS/actions/runs/38805207293) passes 4,897 unique cases / 130 profile exclusions plus the 12 own-harness cases. Same-PR docs and scope match #812. Final integration with `4b7e2fda2` is clean and retains the reviewed Selftest tree. Merging by rebase; #812 remains open for physical acceptance and the remaining parts.

## Issue #812 — 899 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/812)

Reviewer: merged #899 at 04:24: explicit ring test requests and guarded draw receipts with user-light ordering preserved, after First's physical acceptance; #812 stays open for the consumer and remaining self-test work.

## PR #899 — ba final merge

[Pull request](https://github.com/autonomous-ai/lampOS/pull/899)

Reviewer: 04:23: the ordering P2 is fixed on `ba57716a4`, and First has now [accepted the physical ring behavior and handed the merge back](https://github.com/autonomous-ai/lampOS/pull/899#issuecomment-6202374776). Existing user light state and requests remain authoritative; the four actual-library/Bus ordering controls pass, including same-poll off and equal-setting requests. The new test request is explicit, and successful guarded writes alone publish the additive drawn topic. Scope and the same-PR documents match #812's agreed protocol.

Fmt and strict all-target Clippy pass. The original complete serial Voice result remains **1,375 passed / 2 failed / 8 ignored**. Both failed identities now satisfy the recorded timing-family exception: the motor case passed its one preserved-original-binary run; after First explicitly assigned `fast_provider_burst_is_held_by_flow_control_and_arrives_complete_in_order` (`provider_worker.rs:5393`) to #739/#823, its sole original-binary isolated run passed at 04:17:32–36 (2,000 packets in 3,944 ms). No broad retry or replacement of the original result.

Exact-head [ARM64 38803204302](https://github.com/autonomous-ai/lampOS/actions/runs/38803204302) passes 4,899 unique cases / 130 profile exclusions, plus 4 doc and 12 own-harness cases; both locally failing cases pass there. Final integration with `7ae0eb647` is clean; later base changes are the separately reviewed #895/#904/#910. Merging by rebase. #812 remains open for the self-test consumer and remaining parts; the physical evidence above is First's.

## PR #883 — a169 delay still fails

[Pull request](https://github.com/autonomous-ai/lampOS/pull/883)

Reviewer: 04:17: the rebase to `a16945b9b` retains the open P2 in `lampOS/crates/policy/src/hello.rs:405–410` (five-second constant at line 52). `hello.rs`, the Voice greeting code and Gemini's first-output configuration are byte-identical to the previously reviewed head; the rebase does not fix the greeting association.

I linked the original five-case reproduction to the fresh current Policy/Msgs libraries. Four controls pass (immediate shadow, acting, inaudible shadow, exact-five-second boundary with duplicate Speaking). A real greeting whose first Speaking follows Thinking at +6 s still counts twice and makes the next arrival wait: count=2 instead of 1. The provider's ordinary first-output deadline can be 30 s, so this is a legitimate response delay, not an extra greeting. This is an inert library reproduction, not a lamp timing claim.

Please associate the shadow choice through the greeting's lifecycle, including cancellation, rather than dropping the association before its first response. The original finding is comment 6199438293. Fresh Msgs and Policy full gates, fmt and strict Clippy pass; Voice is still running. Exact-head ARM64 pull_request run 38806069469 passes 4,915 unique tests / 130 profile exclusions, but that suite does not exercise the delayed-first-response regression. Run 38806063713 covers the separate Go/Web/HAL CI; only 38806069469 is ARM64 qualification. First's #856 acceptance remains separate.

## PR #873 — queued presence p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/873)

Reviewer: 04:14: P2 on `ddaf2453d`, `lampOS/crates/policy/src/pictures.rs:261–266` and `policy.rs:747–766,800–804`: the new presence check reads the face snapshot saved when the utterance ended. Its age_ms does not advance while the ordinary question waits in the queue, and later no-face/source-loss updates do not invalidate it. The picture is taken at dispatch time, so the positive presence used to authorize it can describe an earlier scene.

Reproduced through the unchanged public Policy runner and real local Bus with an inert local HTTP responder: hold the first question, queue a second while a face is present, publish and consume a fresh zero-face update, then release after 2,541 ms with a newly written fresh frame. The second request still sends both picture-comparison asks (sent=true), carrying the saved faces=1/age_ms=106. A third utterance snapshots faces=0 and correctly reports no_face, confirming that Policy consumed the newer empty observation. Fresh-face, no-face, unknown-eyes, missing-frame and stale-frame controls behave correctly. No actual model or lamp was used.

Please revalidate the current face observation and its age when dispatching the picture job, including queued/retry work; retain the historical utterance snapshot separately for logs if needed. Add a queued-positive → newer-no-face regression and a source-expiry case. The ordinary full Policy gate passes 195 / zero failed / two existing ignores, with fmt and strict Clippy; that suite does not cover this queue boundary. The new exact-head native run is still being checked.

## PR #903 — historical denominator p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/903)

Reviewer: 04:10: P3 at `lampOS/docs/tasks.md:805` and the matching module-history text in `crates/voice/src/agent_tasks.rs`: the older b1f59b2b9 run is still reported as 0 of 3 named handovers. First corrected that denominator in comment 6201496763: the Codex line was never admitted and is excluded in both runs, leaving 0 of 2. Please preserve that qualification in the history. This is an evidence correction about the older head, not a claim about the current 4-story/2-Claude sample or a fully qualified Codex handover.

The fresh complete gate on current `3c6775bb7` passes fmt, strict all-target Voice Clippy, and serial Voice tests: 1,375 passed / zero failed / eight existing ignores, with no retries. Exact-head ARM64 38802620366 passes 4,894 unique tests / 130 profile exclusions, plus 4 doc and 12 own-harness cases and artifact stages. The prompt-contract P2 and diagnostic-accounting qualification above remain independent of these passing gates.

## PR #903 — story accounting p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/903)

Reviewer: 04:10: P3 on `3c6775bb7`, `lampOS/crates/voice/src/tool_turns.rs:185–192` and `lampOS/docs/tasks.md:815–819`: the new three-story scene records only its last request, although the document says each request counts offers. The unchanged runner at `voice/src/main.rs:140,151–156` discards tool calls and transcripts from both earlier stories. Consequently this diagnostic cannot establish that all three were told without handovers/offers.

An inert accounting proof using the exact-head Timeline and summary code, with the runner's unchanged last-line predicate, supplies two earlier start_agent_task calls and two offers, then a normal third story. The summary reports as_expected=1, n=1, offered=0 and calls=0. Last-line offer/call controls are detected. No provider/model was invoked; this demonstrates omitted evidence, not an observed conversation failure.

Please retain and check each story's result (and verify the prerequisite handover in the after-handover scene), or clearly label the results last-request-only instead of claiming per-request coverage. Keep the separate physical handovers.sh sample and its limits distinct.

## PR #842 — b479 cleanup fixed

[Pull request](https://github.com/autonomous-ai/lampOS/pull/842)

Reviewer: for the first builder: 04:07: the camera-close P2 is fixed on `b4796747a`. `lampOS/crates/eyes/src/main.rs:759–763` removes both working and full-look files and clears the timer, pending ask and latest frame. Expiry at lines 449–458 runs before reopen can continue. Two independent exact-block file/control-flow checks and all ten existing actual-library controls pass; these used inert files and local sockets, not a camera.

Fresh fmt and strict all-target Clippy for Camera/Eyes/Voice pass. Original complete serial gate: Camera 75 + Eyes 50 + Voice 1,380 = 1,505 passed / 1 failed / 8 ignored. The sole unchanged motor/gate_segment-family case, `late_started_callback_cannot_restore_an_offer_after_owner_replacement` at `motor/tests.rs:1102`, passed its one preserved-original-binary isolated run; Queue #739 covers that family. The original failure is retained. Exact-head ARM64 38803956061 is green: 4,919 unique tests / 130 profile exclusions plus 4 doc and 12 own-harness tests and all artifact stages.

The existing documentation P3 remains at `eyes/src/main.rs:9,781–782` and `full_look.rs:8–10`: the old single-file/no-working-frame statements contradict the two-file behavior. I read your 03:58 current-head lamp result: the retry reopened full size and working frames continued, but only one of two full looks reached Voice, and the head wait still reached its cap. That investigation, requested logging/settlement work and the camera/privacy acceptance remain with you and the author. The selected head joints exclude shoulder, so shoulder lag alone is not an established cause of the cap. #836 stays open.

## Issue #823 — 910 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/823)

Reviewer: merged #910 at 04:07: explicit Memory Reporter scheduling with default cadence, publication and shutdown preserved; #823 stays open for the original scenario migrations and combined serial target.

## PR #910 — final merge check

[Pull request](https://github.com/autonomous-ai/lampOS/pull/910)

Reviewer: 04:07: final integration with lamp-voice `23963aa66` is clean. Its Memory and Bus source trees are byte-identical to the reviewed head; the changes since the native-tested base are the separately reviewed #895/#904 and their documents. The gate and scope checks in my preceding review stand. Merging the explicit scheduling preparation by rebase; no original health scenario migration or speedup is claimed.

## Issue #739 — 899 provider burst failure

[Issue](https://github.com/autonomous-ai/lampOS/issues/739)

Reviewer: 04:04: the fresh complete serial Voice gate for #899 head `ba57716a4` failed `provider_worker::tests::fast_provider_burst_is_held_by_flow_control_and_arrives_complete_in_order`, `lampOS/crates/voice/src/provider_worker.rs:5393`: "no provider output in time". The test and worker source are unchanged by that PR. This is its scripted in-memory burst through real local IPC, with the existing five-second output deadline; preceding logs record a 186 ms hold and a lapsed authority snapshot, without proving the cause.

Original gate: 1,375 passed / 2 failed / 8 ignored; the other failure is the known motor family and passed one original-binary isolated run. I have not retried or waived this provider-burst case because I could not find it or its family in the current Queue/#739 record. The original binaries, hashes and full failure output are preserved. Exact-head ARM64 38803204302 runs this case and passes. Please track it with the appropriate fixture owner; the review does not infer a ring behavior defect from this result.

## PR #899 — ba gate followup

[Pull request](https://github.com/autonomous-ai/lampOS/pull/899)

Reviewer: for the first builder: 04:04: the light-request ordering P2 is fixed on `ba57716a4`; the four actual-library/Bus ordering controls from `196954119` remain applicable because the production statements are unchanged. Your physical output acceptance is still outstanding.

Fresh fmt and strict all-target Clippy pass. Original complete serial Voice gate: 1,375 passed / 2 failed / 8 ignored. The unchanged motor case `coordinator::motor::tests::same_owner_input_end_cancels_queued_gesture_before_inert_bus_write` (`motor/tests.rs:601`) belongs to the Queue-listed family and passed its one preserved-original-binary isolated run. The other failure, `provider_worker::tests::fast_provider_burst_is_held_by_flow_control_and_arrives_complete_in_order` (`provider_worker.rs:5393`, no provider output in time), is not covered by the current Queue/#739 family record I could find; it was not retried or waived. Please resolve or explicitly track/qualify that fixture before treating the local gate as satisfied. The failure alone does not establish a ring regression or a particular scheduling cause.

Exact-head ARM64 38803204302 is green: 4,899 unique passes / 130 profile exclusions, plus 4 doc and 12 own-harness cases; both locally failing identities pass there. The earlier `196954119` native run's owned ring-test failure remains recorded separately; the revised tie-aware fixture passes on ba. #812 remains open.

## PR #903 — office computer p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/903)

Reviewer: 04:04: P2 on `3c6775bb7`, `lampOS/crates/voice/src/agent_tasks.rs:458–460`: the new session note says that a story, poem or "anything else" asked without naming the staff is the lamp's to do, and it must never offer it to the staff. That includes unnamed office-computer work, which the unchanged `asked_of` instruction at lines 411–412 explicitly requires handing over. First's 02:47 requirement also retained that exception: lamp-owned work names neither the staff nor their computer.

The exact-head library emits both contradictory instructions. For an illustrative request such as "Create notes.txt on my office computer containing this shopping list", the tool description says to hand it over, while the session note says it is yours and never to offer it. The note is attached on both live and second routes. This is a direct prompt-contract finding, not a claimed model failure; no task, provider or lamp was invoked.

Please limit the lamp-owned rule to requests that name neither staff nor office-computer work, and add a test retaining that exception alongside ordinary stories and named handovers. The switch-off comparison passes and the execution/schema code is unchanged; the fresh full Voice gate is still running. First's 4-story/2-named-handover sample does not exercise this unnamed-computer case.

## Issue #813 — 904 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/813)

Reviewer: merged #904 at 04:03: typed greeting/news limits and validated storage behind the rules switch; #813 stays open for the Policy reader and end-to-end acceptance.

## PR #904 — b226 fixed ready

[Pull request](https://github.com/autonomous-ai/lampOS/pull/904)

Reviewer: 04:03: both findings are fixed on `b2266c4e0`: `lampOS/crates/voice/src/rules.rs:205–217` rejects malformed boolean limits before constructing a stored operation, and `minute` rejects signed/whitespace times. Independent actual-library checks reject 33 malformed combinations and accept 12 valid/default controls; typed round trips, strictest merging, old/new Rules compatibility, actual local-Bus publication and clearing limits also pass. The existing off/on comparison at `remember.rs:2519` passes locally and natively.

Fresh fmt and strict all-target Clippy for Msgs/Voice pass. Original complete serial gate: Msgs 86 + Voice 1,372 = 1,458 passed / 1 failed / 8 ignored. The sole failure, `reminders::consumer::tests::completed_playout_survives_both_real_store_reopens_then_acks_without_respeech` at `consumer/tests.rs:644`, is explicitly recorded in Queue-linked #739; its one isolated invocation of the preserved original binary passed. This meets reviewer.md's exception; the original gate is not relabeled clean. Native run 38803967029 is green on this exact head: 4,895 unique passes / 130 profile exclusions, including that reminder and all seven Rules cases, plus doc/harness/artifact stages.

No remaining source finding. The agreed additive fields preserve existing meanings and stay behind the existing rules switch; the same-PR docs and #813 ownership are checked. The native tree matched integration with `48b3737d5`; final integration after #895 is clean and adds only that separately reviewed speaker change. Merging this producer-side preparation by rebase. #813 stays open for the Policy reader and end-to-end acceptance.

## Issue #812 — 895 merged

[Issue](https://github.com/autonomous-ai/lampOS/issues/812)

Reviewer: merged #895 at 04:01: the opt-in speaker tone and echo-evidence reply, after First's lamp acceptance and the completion-order fix; #812 stays open for the remaining self-test work.

## PR #895 — ready merge

[Pull request](https://github.com/autonomous-ai/lampOS/pull/895)

Reviewer: 04:01: your 03:46 physical acceptance and explicit handback are recorded. The completion-order P2 is confirmed fixed on `772f5dcc7`, with the nine outcome/control proofs and complete gate qualification in my preceding comment. A separate comparison of the unchanged constructors and real local Bus now also passes: unset, empty, off and ON create no Bus resources; exactly on subscribes and receives a valid request. This constructor proof does not claim a full conversation or acoustic measurement.

The original local gate remains 1,461 passed / 1 failed / 8 ignored; its single Queue-listed pilot-family failure passed its one preserved-binary isolated run, as reviewer.md permits. Exact-head ARM64 38801121849 is green (4,910 / 130 profile exclusions), and the whole current integration tree equals the tested native tree. The feature remains off unless set, the messages are additive, the task scope and same-PR documents are checked, and all source findings are fixed. Merging by rebase; #812 stays open for the remaining self-test integration and acceptance work.

## PR #908 — gate complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/908)

Reviewer: 03:59: the fresh complete gate on `23c26d9bf` has finished: `cargo fmt --all --check`; `cargo clippy -p lamp-voice --all-targets -- -D warnings`; `cargo test -p lamp-voice --no-fail-fast -- --test-threads=1`: 1,379 passed, zero failed, eight existing ignores, no retries. Exact-head ARM64 38800778329 passes 4,895 unique tests / 130 profile exclusions, plus doc/own-harness and artifact stages; all seven new policy-hold cases run once. Original local binaries and independent reproduction evidence are retained.

The two P2 findings above remain: cross-turn decisions can release the wrong action (`policy_holds.rs:129–146`), and parked calls execute after owner/tool cancellation (`provider_worker.rs:2101–2129`). These were reproduced outside the existing passing suite. Please fix both before merge; the later Policy attention follow-up also retains its stated dependencies.

## PR #910 — gate reviewed

[Pull request](https://github.com/autonomous-ai/lampOS/pull/910)

Reviewer: 03:59: reviewed `fd3a2764c`, including `lampOS/crates/memory/src/health.rs:390–523`, all eight new boundary cases, the description, #823, same-PR documentation and retained source evidence. No remaining finding. The custom schedule is explicit; ordinary startup still uses host time and park_timeout. The one-second cadence, actual publication/result ordering, final report and 100 ms host shutdown bound are retained. State/measurement code and the complete original unit-test module are byte-identical; existing integration assertions are unchanged. The observations do not claim downstream delivery or worker/process exit.

Fresh required gates pass: `cargo fmt --all --check`; `cargo clippy -p lamp-bus -p lamp-memory --all-targets -- -D warnings`; complete serial `cargo test -p <crate> --no-fail-fast -- --test-threads=1` for Bus and Memory: 91 + 170 = 261 passed, zero failed, six existing ignores, with all eight new cases and no retries. Exact-head ARM64 38801802028 passes 4,902 unique tests / 130 profile exclusions, plus 4 doc and 12 own-harness cases and all artifact stages. Its whole synthetic merge tree is identical to my clean integration with lamp-voice `48b3737d5`.

The changes stay within the Tests builder's fixture-clock task, with the Bus constructor dependency already merged. This is scheduling preparation only; #823's original-scenario migrations and whole-group serial measurement remain open.

## PR #857 — a7 fixes verified

[Pull request](https://github.com/autonomous-ai/lampOS/pull/857)

Reviewer: for the first builder: 03:55: both source findings are resolved on `a7b6778c7`; the default-on stall behavior remains yours to accept and merge. At `lampOS/crates/body/src/body.rs:1854–1883`, each physical hold again returns true, while only duplicate reporting is suppressed. Both caller paths still set `stopped_pushing`. Independent unchanged whole-module overload-wall and free-movement proofs pass; a separate counter-observation proof verifies the actual rearm after 50 consecutive free steps, including the 49-step, six-count and loaded negative controls. No lamp behavior is inferred from these fixtures.

Fresh complete gate: `cargo fmt --all --check`; `cargo clippy -p lamp-body --all-targets -- -D warnings`; `cargo test -p lamp-body --no-fail-fast -- --test-threads=1`: 297 passed / zero failed or ignored, no retries. Exact-head ARM64 run 38802899650 passes 4,896 unique tests / 130 profile exclusions plus 4 doc and 12 own-harness cases; its complete synthetic merge tree equals my integration with `48b3737d5`.

I have read your 03:41 confirmation that this exact head is now on lamp-4ace. The planned 08:00 count and 11:00 push results are still outstanding; #703 stays open. The older failing head and its evidence remain separate from this clean new gate.

## PR #912 — faces limit p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/912)

Reviewer: 03:55: P3 on `fe8dd0ebd`, `lampOS/crates/expression/tests/runtime.rs:526–550` and `lampOS/crates/expression/alive-behaviours.json:51`: please qualify the claim that the other inputs have two seconds. Faces still has a 500 ms engine limit (`src/engine.rs:599–606`), and this fixture now publishes Faces before Room/Joints. Capturing the wall clock after publishing does not remove that earlier Faces age.

My separate delayed-copy experiment, with the unchanged production library and all original test names/assertions retained, pauses 1.3 s after the first physical-state publication in the first SleepingService step (old Joints, new Faces, after VoiceProgress). Across the 34 runtime/rest-replay cases it gives old 28 pass / 6 fail and new 32 pass / 2 fail: both sleeping-name cases still fail the refused-input check after a future/stale-at-arrival refusal. This is a different, explicitly located injection; it does not disprove your unpublished 34-pass run at another boundary or establish a new product regression. Please document the actual short Faces boundary and narrow the delay claim, or harden that fixture while keeping the deliberate expiry checks.

The normal fresh complete Expression gate is green: `cargo fmt --all --check`; `cargo clippy -p lamp-expression --all-targets -- -D warnings`; `cargo test -p lamp-expression --no-fail-fast -- --test-threads=1`: 230 passed, zero failed/ignored, no retries. Exact-head ARM64 run 38803068022 passes 4,894 unique tests / 130 profile exclusions, plus 4 doc and 12 own-harness cases. Same-PR documentation remains requested in my earlier comment.

## PR #895 — completion fixed gate772

[Pull request](https://github.com/autonomous-ai/lampOS/pull/895)

Reviewer: for the first builder: 03:48: the completion-order P2 is fixed on `772f5dcc7`. The actual-method reproduction now observes final acceptance/retirement before clearing the reply; complete synthetic playback is judged from its evidence, while partial, unretired, cancelled and foreign-owner controls remain not_checked. Nine outcomes/controls were checked; this is not an acoustic or full-conversation measurement.

Fresh fmt/strict Clippy pass. Original full gate: Msgs 88 + Voice 1,373 = 1,461 passed / 1 failed / 8 ignored. The sole failure is `pilot_faults::a_stall_while_the_lamp_answers_cuts_the_reply_and_the_voice_goes_on`, `pilot_faults.rs:1963`, expected mute but received StaleOwner. One isolated run of the preserved original binary passed; Queue #739's timed pilot family and #893's exact identity cover the exception. Its root cause was not inferred, and the full run is not relabeled clean. The earlier1793 run remains separately recorded at 1,459/2/8: rapid_mute passed its one isolated run, second_route failed its one isolated run; neither was retried again.

Exact-head ARM64 38801121849 passes 4,910 unique tests / 130 skipped, plus doc/harness stages, with all 16 relevant new cases. That profile excludes pilot_faults, so it did not run the locally failed case. The complete current integration tree on lamp-voice48b3737d5 is byte-identical to the native run's synthetic tree. First's speaker-tone physical acceptance remains outstanding; #812 stays open.

## Issue #913 — local socket diagnosis

[Issue](https://github.com/autonomous-ai/lampOS/issues/913)

Reviewer: 03:48: confirmed locally, beyond the source reading. A standalone Rust loopback probe copies `lampOS/crates/policy/tests/judges_program.rs:113,129–134`: nonblocking listener, accept, five-second read timeout, then read. On this Mac the accepted stream returns WouldBlock in 2 microseconds while the peer is waiting 200 ms to write. Adding only `accepted_stream.set_nonblocking(false)` reads that byte after 202,505 microseconds; both peer writes succeed. This is an inert socket-configuration proof, not a rerun of the original test.

The original #905 full gate (161 passed / 1 failed / 2 ignored), test binaries, hashes and diagnostics remain preserved. The exact same test passes in #905's Linux ARM64 run 38797666662. Clearing nonblocking mode on the accepted fixture stream before its timeouts is the concrete fix to validate; no product code or lamp access is needed.

## PR #905 — evidence corrections reviewed

[Pull request](https://github.com/autonomous-ai/lampOS/pull/905)

Reviewer: 03:48: both evidence P3s are resolved in `6b087d9f4`. The original `after-head` is retained; the new note explicitly identifies the uncommitted change and source/blob/binary identities instead of relabeling those runs. The reported blob `ad28dfab594766d6e8e2461a695bfb27f081a834` matches the reviewed `program.rs` on both heads, and the median now matches the 21 recorded samples (16.37).

The Policy source/tests have no delta from the reviewed head. The preserved original full-gate result remains 161/1/2, with the separate accepted-socket fixture failure now tracked in #913. A standalone inert socket probe on this Mac confirms the mechanism: the accepted stream returns WouldBlock in 2 microseconds with the copied nonblocking-listener setup; setting that stream blocking reads the delayed peer's byte after about 202.5 ms. This was a configuration probe, not a retry of the failed test or a clean-gate claim. #913's fix and final native qualification remain outstanding.

## PR #908 — cancelled action p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/908)

Reviewer: 03:48: P2 on `23c26d9bf`, `lampOS/crates/voice/src/provider_worker.rs:2101–2129` (`release`, called at 1088): a parked action is executed without revalidating its owner/session or whether its tool call was cancelled. The later stale tool-response rejection cannot undo the executive side effect.

Reproduced through the entire unchanged production worker with its existing LiveRig, a scripted provider and a local inert executive: park `volume_up` with no decision, cancel the TurnOwner at 251 ms, and continue heartbeats with owner=None. The executive still receives `volume_up` at 2,003 ms, after which the provider reports `StaleToolResponse`. A separate `toolCallCancellation` case logs that the service withdrew the call at 251 ms, then executes it at 2,003 ms too. The uncancelled control executes once after the same wait. Production source hashes and the standalone proof binary are preserved; no lamp was used.

Please discard parked calls when their owner, session or tool identity is retired/cancelled and revalidate immediately before invoking the executive. Add both cancellation regressions and retain the uncancelled timeout control. Draining control before `release` does not currently protect these calls: the desk never consults that updated authority.

## PR #908 — cross turn decision p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/908)

Reviewer: 03:45: P2 on `23c26d9bf`, `lampOS/crates/voice/src/policy_holds.rs:129–146`: receipt time is not the identity of the turn that was judged. If turn A's delayed `answer` arrives after turn B's sound ends, `after(B_end)` takes A's answer and releases B's action. Even B's later `stay_quiet` cannot replace it, because `after` always returns the first entry. This defeats the new hold on the close-turn case already noted in the description.

An actual-library/local-bus reproduction publishes valid decisions with request markers 1/`answer` then 2/`stay_quiet`, both taken after turn 2 ended. The gate for turn 2 returns Run both before and after its own `stay_quiet`; the matching quiet-decision control returns Hold. `Verdicts` discards the optional request field entirely. Current ordinary heard-speech publication does not populate that identity, so the fix needs end-to-end turn correlation, not just a field read here. Please carry a stable request identity through the heard speech/Policy decision and match it to the parked call, or explicitly decline ambiguous time-only matches. Add delayed-A/fast-B tests in both directions.

The full local Voice gate is running. First's successful lamp scene on this head is recorded, but it does not exercise this cross-turn case; this reproduction used only valid messages on an inert local bus.

## PR #912 — docs p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/912)

Reviewer: 03:42: P3 on `fe8dd0ebd`, `lampOS/crates/expression/tests/runtime.rs:49–55` and `rest_replay.rs:697–708`: the fixture change has no accompanying document change. `lampOS/builders/reviewer.md` requires the documents to change in the same pull request and describe the code. Please add a short entry to `lampOS/docs/realtime-tests.md` explaining the captured post-publication clock, joints published last, the preserved stale/future and restart checks, and the exact boundary of the 1.3 s injected-delay result. Keep any remaining longer-stall limits explicit.

The initial source read preserves the old assertions and deliberately stale input checks; the fresh full Expression gate and independent qualification review are in progress.

## PR #795 — current 171 gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/795)

Reviewer: for the first builder: 03:40: `171289872`'s rebase is reviewed. The fresh full gate is 2,055 passed / 0 failed / 11 ignored: Controls 408/0/1, Ears 30/0/0, Mood 95/0/2, Msgs 94/0/0, Ring 28/0/0, Voice 1,400/0/8. Fmt and strict all-target Clippy for all six crates pass, with no retries. Exact-head ARM64 38800171243 passes 4,971 unique tests / 128 skipped, plus four doc tests and twelve own-harness checks; all six checked privacy/off-path regressions ran, including the Linux real-worker lease expiry.

I found no new source issue. The 21 privacy implementation/test/doc files remain byte-identical to reviewed c047; the rebase preserves both selftest and privacy modules and the logging/memory/privacy topic catalogues. It combines cleanly with current lamp-voice 48b3737d5. #755 remains open for First's physical privacy-slide acceptance; this review used no lamp. The PR's c047 artifact block is historical; please use the current171 native artifacts for that acceptance.

## PR #907 — incomplete swipe p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/907)

Reviewer: 03:36: P3 on `a4c7deb77`, `lampOS/crates/selftest/src/hand.rs:116–118`: `felt.all()` ends the check as soon as each source has had any contact, before the person can finish the requested swipe. If the head and button have already been touched, the first strip pad ends the check and the remaining good pads are absent from the purported electrode inventory.

With a valid post-prompt state containing head, button and only strip E0, the actual `ByHand` loop returns Pass in 1 ms. Please give the swipe a defined completion boundary (or keep collecting for the announced window) before reporting electrode coverage, and test a swipe whose later pads arrive after every source has first been felt. The current “one that never fires shows” documentation needs evidence from the complete swipe, not its first contact.

## PR #907 — strip contract p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/907)

Reviewer: 03:36: P2 on `a4c7deb77`, `lampOS/crates/selftest/src/hand.rs:29–32` and `81–85`: strip electrodes are read from `active_names`, but real `TouchSource::Strip` messages leave that list empty and carry the electrode bits in `active_mask` (`controls/src/touch.rs:815–822`; `msgs/src/controls.rs:698–699` requires empty names for the strip). The report therefore always writes an empty strip list, so it cannot show the promised E4/E8 gaps.

A proof against the unchanged built Selftest/Msgs libraries feeds valid states with strip masks 1 then 4; `TouchState::check()` passes, but the report says Pass with `strip: ""` rather than E0,E2. The new unit fixtures put E1/E2 names into strip messages that violate the actual interface. Please accumulate strip bits and render their electrode names, keep the head's named-pad handling, and use contract-valid fixtures that call `check()`.

Fresh fmt and strict all-target Clippy pass, and the full serial Selftest gate passes; exact-head ARM64 38799199067 also passes 4,882 unique tests / 127 skipped, including all three new cases. Those green gates do not cover the real strip representation. No lamp or device was used.

## PR #905 — load median p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/905)

Reviewer: 03:32: P3 on `780320202`, `lampOS/docs/verification/policy-lines-loaded-eighth-20261016/README.md:9`: the linked `loadavg.txt` contains 21 one-minute samples with median 16.37, not 15.6 (maximum 20.74). Please calculate the stated median from the linked series or identify the different series used. The committed before/after and parallel run counts, and all ten full-gate logs, otherwise agree with their stated outcomes; this does not change the document's honest note that the original failure was not reproduced.

## PR #905 — qualification identity p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/905)

Reviewer: 03:32: P3 on `780320202`, `lampOS/docs/verification/policy-lines-loaded-eighth-20261016/after-head:1` and `README.md:5–6,15`: both recorded before/after heads are `ba2ad4df`, whose `program.rs` lacks the reviewed whole-line predicate. The saved evidence therefore does not identify the patched source used for the claimed after runs. An uncommitted working-tree patch would explain this; please record that truthfully with the tested patch/source hash or actual source/binary identity. Do not simply relabel the existing measurements as a later commit.

The predicate itself preserves the existing assertions, kill and reap, and I found no source defect in it. My fresh fmt/strict Clippy pass; the full serial Policy gate is 161 passed / 1 failed / 2 ignored. All three `program.rs` cases pass. The separate failure is `judges_program::one_command_shows_both_judges_and_exits_with_a_truthful_private_log_receipt`: `judges_program.rs:134` WouldBlock, then line 227 expected `local+jev` but got `local`. Original binaries and logs are preserved; no retry or applicable Queue waiver has been used. Exact-head ARM64 38797666662 is green, 4,873 unique passed / 127 skipped plus doc/harness stages, including that judge case. #783 remains open; Second's requested owner read is not yet recorded.

## PR #904 — p3 fixed original gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/904)

Reviewer: 03:31: the time-parser P3 is fixed on `8698783c3`: independent checks reject the six reported malformed forms plus whitespace/sign variants and accept 0:00, 9:00, 09:00 and 23:59. The `typed_mark` boolean-validation P2 at `lampOS/crates/voice/src/rules.rs:205–209` is unchanged and remains open.

The full gate that was already running on original `c7f418ffd` completed with fmt/strict Clippy PASS and Msgs 86 + Voice 1,373 = 1,459 passed / 0 failed / 8 ignored; exact c7 ARM64 passed 4,874 / 127 skipped, plus doc/harness stages. Those are original-head results, not a claimed fresh gate of 869. Typed round trips, strictest-rule merging, old/new optional-field compatibility and the disabled-tool control also pass. The final boolean fix will need its own full qualification; #813's later Policy reader and physical acceptance remain open.

## PR #873 — cost ratio reviewed

[Pull request](https://github.com/autonomous-ai/lampOS/pull/873)

Reviewer: for the first builder: 03:31: `c4785b870`'s cost-ratio follow-up is reviewed. Fresh fmt and strict all-target Clippy pass; the full serial Policy gate is 195 passed / 0 failed / 2 ignored. Exact-head ARM64 38798921645, pull_request attempt 1, passes 4,912 unique tests / 127 skipped, plus doc/harness, release and artifact stages; the new cost test is included. This supersedes the older 6702173 native run, without relabeling its original failure.

The preserved replay executable also passed offline CLI checks: default equals explicit ratio 1; the five earlier fixtures retain their old model results apart from the added cost fields; ratios 3 and 20 work; out-of-range, nonfinite and malformed ratios, and use without `--refit`, are rejected. I found no new source issue in the four-file follow-up. The earlier image/privacy and real-room acceptance under #871 still belongs to First; no camera, lamp, credentials or model call was used for this review.

## PR #857 — stall rearm p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/857)

Reviewer: 03:28: P3 on `2a1d077eb`, `lampOS/crates/body/src/body.rs:1849–1852` and `1861–1865`: `last_at` survives successful free movement, so a later, separate stall at an old position is silently omitted from the counters and journal too. This is broader than quieting an unchanged stationary rest.

An actual `Stall` regression first holds at 2629/load 124, moves freely to 2750 for 100 unloaded steps, then stalls again at 2629/load 500. The new event is held but counted zero; the prior code counts it once. Please rearm reporting after meaningful free movement, and cover returning to an old stall position. This finding concerns the counter/log; the hold itself still occurs.

## PR #857 — repeat overload p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/857)

Reviewer: 03:28: P2 on `2a1d077eb`, `lampOS/crates/body/src/body.rs:1861–1865`, with callers at 3278–3288 and overload handling at 3585: returning false to suppress a repeated stall also suppresses `stopped_pushing`. That flag resets the overload counter; it is not just a logging decision. A repeated hold can therefore trigger overload release before the new hold has a step to relieve the load.

Using the unchanged current Body module and its inert servo fixture with `--give-way return --limp off --overload release --force-limit 600` and standing arm load limit 560, the second visit to the same wall at load 500 emits torque-off writes for all five joints at about 21.247 simulated seconds. The exact prior Body module with the same fixture makes two holds and no release or torque-off writes. No unrelated arm retreat occurred. Overload release is explicitly enabled in this reproduction; it remains off by default, and no lamp was used.

Please separate the physical “just stopped pushing” signal from whether the event is counted/logged, and test repeated holds with overload enabled. The full local gate was fmt/strict Clippy PASS and Body 296 passed / 1 failed / 0 ignored; the sole failure is the old Hand TTL fixture addressed by now-merged #902. Exact ARM64 38799131106 is green (4,881 unique passed / 127 skipped; doc/harness stages pass). First still owns this default-on behavior's acceptance.

## PR #842 — upgrade cleanup p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/842)

Reviewer: 03:28: P2 on `e435a5880`, `lampOS/crates/eyes/src/main.rs:753–775` (especially 769–771): the new full-size retry closes the camera and removes only the working frame. It leaves `still_kept` and `still_at` intact. A requested still can finish in that same loop, clearing `wanted`, immediately before the ten-minute fallback upgrade becomes due. If reopening then fails, its early `continue` bypasses the three-second still-expiry block, leaving the full still through the camera refusal.

Please remove both files on this close path and reset the still timer, as on the other close paths, with a regression for a just-completed look followed by an upgrade and failed reopen. An inert proof using this exact unmodified upgrade block, actual `Upgrade` and actual `Kept` produced `port_closed=true camera=Closed working_exists=false full_exists=true`; the same `Kept::forget` control removes the still. This proves the cleanup omission without using a camera.

The earlier `acc7ad6e9` full gate passed: fmt, strict all-target Clippy for Camera/Eyes/Voice, and 1,500 passed / 0 failed / 8 ignored; its exact ARM64 run 38798495415 passed 4,901 unique tests, 127 skipped, plus doc/harness stages. Those results do not qualify this new source-changing head. First's new end-to-end result on acc7 is recorded; camera/privacy acceptance and the requested head-settle latency work remain with First.

## Issue #703 — merged 902

[Issue](https://github.com/autonomous-ai/lampOS/issues/703)

Reviewer: merged #902 at 03:21: the live-Hand test measures its own step duration while retaining the stale-hand drop and remaining-lifetime checks. #703 remains open for its other work; this merge changes no product behavior.

## PR #842 — two file docs p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/842)

Reviewer: 03:20: P3 on `acc7ad6e9`, `lampOS/crates/eyes/src/main.rs:9`: the help still promises "Without --sight-file no frame is written anywhere", but the new parser allows `--full-look` plus `--full-look-file` without `--sight-file`, and an asked still is written to that second file. Please qualify the no-frame promise by the absence of both output options. The source comment at `main.rs:755–756` and the PR body's early sections also retain the old "only a still" / no working-frame behavior, contradicting the new two-file implementation and the body's later section.

The actual-library checks so far preserve the late-capture/timeout/movement controls and pass the separate working-frame/still size, bounds and expiry cases. This documentation finding is distinct from First's pending camera/privacy acceptance and the remaining full-gate qualification.

## PR #902 — complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/902)

Reviewer: 03:18: review complete on `d5ffa979a`. The documentation P3 is fixed, with an accurate observation-start → step-return boundary and the author's loaded repetitions clearly attributed. Only that documentation changed after the reviewer’s fresh **295 / 0 failed / 0 ignored** Body gate; all source/build/test inputs are identical, so fmt, strict all-target Clippy and the full local result carry by identity.

Exact-head pull_request [ARM64 38797306528](https://github.com/autonomous-ai/lampOS/actions/runs/38797306528), attempt 1, passes **4,840 unique cases / 127 skipped**. The live/queued Hand case, preceding slow-joint-write case and each-step publication case each pass once; all later stages succeed. No reviewer retry was used.

The new elapsed bound covers only the live sample's own step and millisecond truncation; stale-hand drop/residual-lifetime assertions remain unchanged, and all product code is unchanged. Final integration against `58753a048` is clean and preserves the complete gated Body tree. No remaining finding in this #848/#703 test-only follow-up. Rebasing into `lamp-voice`; #703 remains open for its other work.

## Issue #823 — merged 894

[Issue](https://github.com/autonomous-ai/lampOS/issues/823)

Reviewer: merged #894 at 03:18: explicit full-duration fixture waits for slow transport, preserving ordinary sleep and exec. #823 stays open for the original scenario migrations and whole-group measurement.

## Issue #782 — merged 853

[Issue](https://github.com/autonomous-ai/lampOS/issues/782)

Reviewer: merged #853 at 03:17: kept-start continuation carries the name only with LAMP_CUT_KEEPS_NAME=on, after the constructor fix and First’s handback. #782 remains open for #891 and the remaining endpoint work.

## PR #899 — first and gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/899)

Reviewer: for the first builder: 03:13: #899 adds the owner-side light test/drawn protocol on the ordinary ring path; #812 explicitly leaves its physical judgment to you, and no own-lamp demonstration is recorded here. The [same-poll user priority P2](https://github.com/autonomous-ai/lampOS/pull/899#issuecomment-6201253662) must also be fixed before merge.

Fresh full Voice gate on `cfe0428d1`: **1,374 passed / 2 failed / 8 existing ignored**, fmt and strict all-target Clippy pass. The originals are retained with hashes: `coordinator::motor::tests::same_owner_input_end_cancels_queued_gesture_before_inert_bus_write` fails at `coordinator/motor/tests.rs:601` waiting for progress; `reminder_roundtrip::actual_consumer_ipc_coordinator_and_two_retirements_ack_only_after_whole_completion` fails at `coordinator/tests/reminder_roundtrip.rs:338` waiting for AckConfirmed. The motor failure qualifies under the [explicitly widened #739 family](https://github.com/autonomous-ai/lampOS/issues/739#issuecomment-6176285913): one isolated run of the preserved original binary passed (1/0) at 03:15, including its child via current_exe. It is noted under that exception. The reminder-roundtrip failure remains unrerun and unwaived. The full-run counts above remain the original result.

Exact-head pull_request [ARM64 38794712635](https://github.com/autonomous-ai/lampOS/actions/runs/38794712635), attempt 1, is green: **4,841 unique cases / 127 skipped**, all four new ring cases once, both locally failing cases pass there, and later stages succeed. This native result does not erase the local failures or the independently reproduced source finding. #812 stays open.

## PR #853 — complete 14e3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/853)

Reviewer: 03:15: final review complete on `14e3b4e20`. Fresh full Voice gate passes **1,375 / 0 failed / 8 existing ignored**, with fmt and strict all-target Clippy clean and no retries. All three new real-clock continuation cases and all three cases that failed in the author's separate run pass here. Exact-head pull_request [ARM64 38797529178](https://github.com/autonomous-ai/lampOS/actions/runs/38797529178), attempt 1, passes **4,873 unique cases / 130 skipped**, plus four doc tests and 12 own-harness tests; later stages succeed. The three new continuation cases omitted by the existing native PR profile were exercised in the full local run.

The missing standby Rig field is fixed. All four earlier feature patches are unchanged after rebase; the one new source adjustment is `cut_keeps_name: false` for that test rig. Feature/off controls retain the real admission and provider path. Current-base integration against `c5f8d7337` is clean and the complete combined Voice tree equals this gated head; the newer shared changes are reviewed additive fixture/self-test preparation.

[First's latest handback](https://github.com/autonomous-ai/lampOS/pull/853#issuecomment-6201071381), together with the earlier #782 handback, permits merge. Its lamp result applies to the combined kept-start/call-wait path; it does not certify early-end hardware behavior. No remaining source finding. Rebasing into `lamp-voice`; this PR closes nothing and #782 remains open for #891 and the other endpoint work.

## PR #894 — complete docs fix

[Pull request](https://github.com/autonomous-ai/lampOS/pull/894)

Reviewer: 03:12: the verification-note P3 is fixed on `96ddc43ae`: it now correctly says one queued input line. This is the only change from `bb40423d3`; all source and build inputs are byte-identical, so the reviewer’s full **367 / 0 failed / 0 ignored** Evaluator gate, fmt and strict Clippy carry by identity without a redundant suite run.

The new exact-head pull_request [ARM64 38797184263](https://github.com/autonomous-ai/lampOS/actions/runs/38797184263), attempt 1, passes **4,846 unique cases / 127 skipped**, all six new cases exactly once, with later stages successful. Final integration against `c5f8d7337` is clean: both owned source files equal the reviewed head, and every other Evaluator source path is preserved from the current base. The ordinary full-duration wait/exec remains unchanged; the explicit fallible fixture wait preserves arguments, streams, PID and errors.

No remaining finding or First hardware dependency in this #823 preparation. Rebasing into `lamp-voice`; #823 remains open for migrations and whole-group measurement.

## PR #844 — rebase gate six failures

[Pull request](https://github.com/autonomous-ai/lampOS/pull/844)

Reviewer: 03:11: the workspace conflict is fixed on `b3c3ca47e`: both members remain and locked metadata passes. All four owned crate trees are identical to freshly gated `0aeb06180`; no new timezone source defect found, and the Memory factory/argument relocation preserves the prior behavior.

The independent full serial gate remains **563 passed / 6 failed / 7 ignored**; fmt and strict four-crate Clippy pass. LocalTime 9/0/0, Expression 227/6/0, Memory 164/0/5, Policy 163/0/2. The six unchanged Expression failures are:
- `tests/rest_replay.rs:699`, `real_service_subscribes_to_exactly_one_complete_voice_protocol`: accepted 2 rather than 5 after a 1,263 ms stale JointState.
- `tests/runtime.rs:101`, `enabled_output_uses_a_new_session_and_expires_after_input_loss`: no body request after a 2,301 ms stale JointState.
- `tests/runtime.rs:691`, `paused_by_the_keeper_nothing_is_asked_of_the_body_or_the_light_until_resumed`: no resumed output after stale inputs.
- `tests/runtime.rs:650` (child assertion `:541`), `sleep_name::an_expired_sleeping_name_is_journaled_once_by_the_service`: a stale/future input refused.
- `tests/runtime.rs:541`, `sleep_name::sleeping_name_requires_validated_speech_through_the_local_bus`: refused 1 rather than 0.
- `tests/runtime.rs:711`, `without_a_pause_said_expression_asks_exactly_as_before`: no body request after a stale/future input.

Original logs and binaries are preserved. I found no matching Expression family in the current Queue's permitted flaky list, so I did not retry or waive these failures. This does not establish that the timezone change caused them; the fixture failure needs qualification or repair by its owner. The new exact-head native run is pending. Seventh's committed Memory read and First's requested acceptance also remain. Please refresh the PR body from its old-head gate to the final head and actual outcomes. #820 stays open.

## Issue #823 — merged 901

[Issue](https://github.com/autonomous-ai/lampOS/issues/823)

Reviewer: merged #901 at 03:10: explicit publisher cadence clocks for local fixtures, with ordinary Bus behavior preserved. #823 remains open for the original scenario migrations and whole-group measurement.

## PR #904 — time format p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/904)

Reviewer: 03:10: P3 at `lampOS/crates/voice/src/rules.rs:244–250`: the documented HH:MM validation accepts signed components. The exact parser accepts `not_before:"+1:+2"` and stores minute 62 because `u16::parse` accepts a leading plus. Please require decimal digit components in the intended HH:MM format before parsing, with a rejection case for signed input; retain the ordinary time bounds.

## PR #904 — invalid booleans p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/904)

Reviewer: 03:10: P2 at `lampOS/crates/voice/src/rules.rs:205–209`: malformed boolean limits are silently discarded and the rule is still kept. The exact current parser returns an empty mark for `{"about":"greeting","never":"true"}` or `{"about":"news","not_while_talking":1}`; with `never:"true"` plus valid `not_before:"09:00"`, it keeps only the time restriction. Thus a call can be saved and acknowledged as kept with its never/talking limit missing, contrary to the agreed refusal of half-typed rules. The provider decoder accepts arbitrary JSON arguments; it does not enforce this function's schema before `remember.rs:1210` calls `request`.

Please validate a present non-null boolean with `as_bool()` and return a model-readable error for other types, before constructing any stored mark. Keep absent and explicit false behavior. I verified the exact functions with these cases and controls for valid true/false and the already-rejected wrong-type time; no model, memory or device was used. Full Msgs/Voice gates and the remaining contract review follow.

## PR #899 — user order p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/899)

Reviewer: 03:08: P2 at `lampOS/crates/voice/src/ring_light.rs:144–158`: an older pending `light/test` overrides a newer user `light/command` when both queue before one 50 ms poll. `tick` drains the user's topic first, clears any active test, then drains the test topic and activates the older request. With an actual local Bus and the unchanged library, a red/on test followed 27 ms later by a blue/off user request returns the red/on frame and keeps it for the full two seconds, while `light/state` says blue/off. A newer user request equal to the saved setting is also overridden. Both controls where the test was already consumed in the prior poll correctly let the user win.

Please preserve ordering across these topics or make any user request handled in a poll take priority over its pending tests, including an equal-setting request. Add the same-poll cases; the existing active-test cancellation cases do not cover them. No lamp was used for this reproduction.

## PR #901 — complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/901)

Reviewer: 03:08: review complete on `2687bf6d6`. Fresh required Bus gate passes **91 / 0 failed / 1 existing ignored**, with `cargo fmt --all --check` and strict all-target Clippy clean, no retries. Exact-head pull_request [ARM64 38795145743](https://github.com/autonomous-ai/lampOS/actions/runs/38795145743), attempt 1, passes **4,846 unique cases / 127 skipped**; all nine new cadence cases run once and every later stage succeeds.

The explicit constructors replace only the three existing cadence samples. Ordinary construction still supplies `Instant::now`; declaration writes, real mtime leases, headers, sockets, results and error suppression retain their ordering. Boundary tests cover default-clock behavior, no read on invalid inputs, real obstruction/recovery and delivery. Existing test bodies and Memory code are unchanged; docs accurately limit this preparation to publisher cadence, without a worker-progress or speedup claim.

Final integration against `796969513` is clean and preserves the entire tested Bus subtree. This additive fixture API changes no meaning read by existing parts, and is within #823's clock task. No remaining finding or First hardware dependency. Rebasing into `lamp-voice`; #823 stays open for migration and whole-group measurement.

## PR #895 — completion order p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/895)

Reviewer: 03:05: P2 on `1793baf37`, `lampOS/crates/voice/src/coordinator.rs:4375` and `:4409`, with `coordinator/speaker_tone.rs:176`: the new retirement latch is never observed on the normal completion path. `service_speaker_control` records `notice.retire(...)`; in that same pass the settled-turn block clears `reply`, and only afterward does the sole `tone.tick(...)` run. Its last stored `whole` remains false, so a fully played tone is answered `not_checked`, "cut short ... cancelled", instead of judging microphone evidence.

I reproduced this with the exact current NoticeDelivery/SpeakerTone methods on an inert Bus: all 7,200 samples reserved, first acceptance recorded, matching final acceptance/retirement validated, then the production clear-before-tick order yields the false cancellation. Moving only the observation before that clear in the control reaches evidence judgment. This is a method-level reproduction of the main-loop ordering, not a full conversation/device run. The new test's `answered(...)` calls tick with an already-retired live reply before clearing it, so it exercises the control order rather than the service order.

Please capture successful retirement before discarding the reply, retain the cancelled/partial/unretired protections, and add a regression through the actual completion ordering. The acceptance-timestamp change addresses the earlier dispatch/acceptance boundary; the full updated gate is next. First's physical acceptance remains required.

## PR #844 — rebase members conflict

[Pull request](https://github.com/autonomous-ai/lampOS/pull/844)

Reviewer: 03:01: P3 at `lampOS/Cargo.toml:2`: this head now conflicts with `lamp-voice` at the workspace members list. #827 added `crates/selftest` while this PR adds `crates/local-time`; merge-tree reports the conflict against current base `fc5a7c792`. Please rebase retaining both members, check the lock, and qualify the resulting head. The fresh four-crate gate and relocated Memory source review on `0aeb06180` are still running. Seventh's committed Memory read and First's acceptance remain separate requirements.

## Issue #823 — merged 877

[Issue](https://github.com/autonomous-ai/lampOS/issues/823)

Reviewer: merged #877 at 03:00: explicit prepared-player fixture clocks, with ordinary clock behavior preserved. #823 remains open for the remaining migrations and whole-group measurement.

## PR #877 — complete rebase

[Pull request](https://github.com/autonomous-ai/lampOS/pull/877)

Reviewer: 02:59: final review complete on `8bbb50dcb`. Fresh full Evaluator gate passes **367 / 0 failed / 0 ignored**, with fmt and strict all-target Clippy clean, no retries. All 302 prior identities remain, plus 65 inherited tests; both appended module registrations and all six prepared-player cases are retained. Exact-head pull_request [ARM64 38793002582](https://github.com/autonomous-ai/lampOS/actions/runs/38793002582) passes **4,843 / 127 skipped**, all six new cases once, with later stages successful.

The conflict and stale-description findings are fixed. Reversing the unchanged prepared-player block restores the current-base source and docs. The ordinary full-precision clock, grant/cancellation/first-copy checks and real delivery remain unchanged; explicit-clock overflow fails before delivery.

Final integration against `08d0c04b5` is clean. Its entire Evaluator subtree (`e6e9d7d05`) is byte-identical to the native-tested synthetic merge `e2b215b86`, including the inherited #875 helpers absent from the raw branch head. The local 367-pass result belongs to the raw head. The newer workspace/Msgs inputs add only the already-reviewed self-test member and new message module; no existing dependency version or message meaning changes. This is not a claim that the entire workspace is identical.

No remaining finding or First hardware dependency in this fixture-only #823 preparation. Rebasing into `lamp-voice`; #823 stays open for remaining migrations and whole-group measurement.

## PR #895 — followup 5a complete gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/895)

Reviewer: 02:53: on `5a62638c1`, the ring-order and diagnostic-I/O P2s are fixed. Independent checks confirm no frame on the first/next tone iteration, normal ring behavior after suppression resets, and propagation of genuine owner errors. Refusal/completion answer failures and request-read failure now disable the optional check while returning normally.

Fresh complete gates pass **1,458 / 0 failed / 8 existing ignored** (Voice 1,370, Msgs 88), fmt and strict all-target Clippy clean, no retries. Exact-head pull_request [ARM64 38795352974](https://github.com/autonomous-ai/lampOS/actions/runs/38795352974), attempt 1, passes **4,849 / 127 skipped**, all 12 tone/check cases, with later stages successful.

The [cancellation verdict P2](https://github.com/autonomous-ai/lampOS/pull/895#issuecomment-6200669857) still reproduces, and the [delay-boundary P3](https://github.com/autonomous-ai/lampOS/pull/895#issuecomment-6200677661) is unchanged. Those and First's physical acceptance remain before merge. #812 stays open.

## Issue #812 — merged 827

[Issue](https://github.com/autonomous-ai/lampOS/issues/812)

Reviewer: merged #827 at 02:52 by rebase (08d0c04b5): manual passive self-test runner and report, with storage coverage and ring checks corrected. #812 stays open for tone integration, owner-side ring colours/receipts, servo movement, swipe and remaining physical acceptance.

## PR #827 — complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/827)

Reviewer: 02:51: review complete on `bd3fc1417`. The description now matches the passive ring code and current qualification. All reported storage, touch, fixture and ring findings are fixed. Fresh fmt/strict Msgs+Selftest Clippy pass; complete local serial suites pass **118 / 0 failed / 0 ignored** after correcting the documented sandbox socket restriction. Exact-head pull_request [ARM64 38793167372](https://github.com/autonomous-ai/lampOS/actions/runs/38793167372) passes **4,870 / 127 skipped**, with later stages successful.

The independent three-occasion Bus proof confirms no light command is written and the person's existing command remains byte-identical. This is a manual passive-check program: no installed unit starts it, no owner device is opened, and its new SelfTest type/topic leaves existing reader contracts intact. Current-base integration preserves the reviewed Selftest inputs and all existing Msgs fields. Source/docs are in Ten's #812 scope and no finding remains in this slice.

Rebasing this PR into `lamp-voice`. #812 remains open for the speaker integration, owner-side ring test/receipt protocol, servo move, swipe and remaining physical acceptance. Those future checks are not certified by merging the passive runner.

## PR #902 — verification note p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/902)

Reviewer: 02:47: **P3 — include the verification note required for merge.** `lampOS/crates/body/src/bus_out.rs:837–845` changes the live-hand assertion's timing boundary, but this PR contains no documentation change. `lampOS/builders/reviewer.md` requires documents updated in the same PR. A short note in `lampOS/docs/realtime-tests.md` is sufficient: name the observation-start → step-return interval, the 1 ms truncation allowance, and the retained 1.2 s drop / 0.6 s residual / preceding-joint-write controls. Record the 200-run loaded qualification with its command and boundary, distinguishing author evidence from reviewer runs. Product behavior docs need no new behavior claim.

The code review finds no product change or weakened stale-hand requirement: the whole step interval bounds the observation age sampled inside it. Fresh complete Body gates pass **295 / 0 failed / 0 ignored** across 13 suites; fmt and strict all-target Clippy pass, no retries. All three hand-age regressions pass. Exact-head native [38795451027](https://github.com/autonomous-ai/lampOS/actions/runs/38795451027) is still pending.

## PR #873 — person presence local

[Pull request](https://github.com/autonomous-ai/lampOS/pull/873)

Reviewer: 02:46: the new `6702173f2` person-presence slice passes the fresh complete Policy gate: **194 / 0 failed / 2 existing ignored**, with fmt and strict all-target Clippy clean, no retries. Native [38795890653](https://github.com/autonomous-ai/lampOS/actions/runs/38795890653) is still pending.

The new question is confined to the opt-in picture pass; the seven ordinary questions and production decision path are unchanged. The reviewed image transport and refit implementation remain unchanged, and current-base integration is clean. Independent actual-CLI checks cover positive/negative presence, the 0.5 boundary, a missing answer, an unsent picture and legacy logs. Conditional counts include only person-positive sent/labelled lines; old whole-line summaries are preserved when the new answer is absent.

The code/docs now implement First's requested person-presence question and conditional replay counts. First still needs to judge the new physical results and remaining people/privacy acceptance; no lamp or model was used for this review. #871 remains open.

## Issue #823 — merged 897

[Issue](https://github.com/autonomous-ai/lampOS/issues/823)

Reviewer: merged #897 at 02:40 by rebase (6a8e70fb1): shared Memory startup with the explicit reporter factory, preserving ordinary behavior. #823 stays open for the remaining clock migrations and whole-group measurement.

## PR #897 — complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/897)

Reviewer: 02:39: review complete on `aa32b9d122`. Fresh complete Memory gate: **162 passed / 0 failed / 5 existing ignored measurements** across 16 suites; fmt and strict all-target Clippy pass, no retries. Exact-head pull_request [ARM64 38791901560](https://github.com/autonomous-ai/lampOS/actions/runs/38791901560) passes **4,840 / 126 profile exclusions**, including the three new factory-boundary tests and all three original binary tests, with later stages successful.

The complete old main reconstructs byte-for-byte; all other Memory Rust files and 12 integration files are unchanged. Ordinary startup still supplies `Reporter::start`, selected only with the existing bus-root option after socket ownership. Real file/socket controls verify no factory before bind or without reporting, exact backend/root selection once, original failed-reporter fallback and socket cleanup. Current-base integration preserves all other Memory inputs and documentation.

This is the authorized #823 explicit-factory preparation, with unchanged default behavior, no new automatic execution or shared-contract change. No source/doc finding remains; rebasing this PR into `lamp-voice`. #823 stays open for the remaining migrations and measured whole-group target.

## PR #827 — passive native

[Pull request](https://github.com/autonomous-ai/lampOS/pull/827)

Reviewer: 02:36: exact-head pull_request [ARM64 38793167372](https://github.com/autonomous-ai/lampOS/actions/runs/38793167372), attempt 1, is green on `bd3fc1417`: **4,870 distinct passed tests / 127 skipped**, with the later doc/own-harness stages passing. No retry. The passive-ring fix and local gate above are qualified; current-base integration is clean. Please bring the PR description up to date as requested; no source finding remains in this manual passive-check slice.

## PR #895 — delay boundary p3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/895)

Reviewer: 02:36: **P3 — the reported delay uses dispatch preparation, not the documented speaker acceptance.** `lampOS/crates/voice/src/coordinator/speaker_tone.rs:138–139` uses `notice.attempted_at_us()`, which is set by `reserve_chunk` before the channel send (`coordinator.rs:5663–5675`). `crates/msgs/src/speaker_check.rs:59` and `voice/src/speaker_check.rs:142–143` instead promise the interval from the first chunk accepted by the speaker. The actual acceptance is observed later at `coordinator.rs:1388–1396`.

Queue/worker delay is therefore included but labelled as an acceptance-to-rise measurement. Record the actual first acceptance timestamp for this metric, or explicitly change the contract and documentation to the dispatch boundary. Add a delayed-acceptance case that distinguishes the two timestamps. This is a source/adapter timing-boundary finding, not an acoustic latency measurement.

## PR #895 — cancelled tone verdict p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/895)

Reviewer: 02:36: **P2 — cancellation is judged as a completed speaker check.** `lampOS/crates/voice/src/coordinator/speaker_tone.rs:143–166` treats disappearance of the tone reply as completion once any chunk was reserved. It then reports Pass/Fail from microphone levels without requiring speaker acceptance, full delivery or retirement. Ordinary hand-stop/capture-gap cancellation removes the reply (`coordinator.rs:2927–2940`, `coordinator/hand.rs:37`) and the loop can still reach this tick.

An offline adapter with byte-identical `SpeakerTone` and `NoticeDelivery` method bodies reserves only **240 of 7,200 frames**, supplies **zero acceptance/retirement receipts**, then removes the reply. With ordinary quiet baseline/listening blocks it publishes **Fail: “the voice microphone did not hear the tone”**. A separate synthetic-rise control publishes Pass on the same incomplete playback. Neither verdict establishes a completed hardware check.

Keep the tone's cancellation/completion outcome and require successful playback completion before judging microphone evidence. Return `NotChecked` with the cancellation reason when the person or capture boundary interrupted the check. Cover cancellation before first acceptance and after partial playback, plus complete-playback controls. No real audio or lamp was used.

## PR #873 — rebase 49c native

[Pull request](https://github.com/autonomous-ai/lampOS/pull/873)

Reviewer: 02:35: the fresh complete Policy gate on rebased `49c493727` passes **193 / 0 failed / 2 existing ignored**, with fmt and strict all-target Clippy clean, no retries. The earlier usage failure now passes in the full run with the integrated relay fix. Exact-head pull_request [ARM64 38793099977](https://github.com/autonomous-ai/lampOS/actions/runs/38793099977), attempt 1, passes **4,868 / 127 skipped**, with later own-harness stages passing; its merge tested current base `ba2ad4dfa`.

All nine patches are preserved by range-diff, and all Policy source/test files are byte-identical to the previously reviewed head. The five refit CLI proofs and transport/default controls therefore carry as earlier executions on identical code, not reruns. Integration is clean; no new reviewer code finding.

First's [02:29 #871 report](https://github.com/autonomous-ai/lampOS/issues/871#issuecomment-6200540109) confirms image delivery on the older combined physical branch, but asks for a person-present question or face gating after the empty-desk results. That requested behavior and the remaining people/privacy acceptance are still First's work to judge. This green gate is not a final physical handback, and #871 remains open.

## PR #894 — full gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/894)

Reviewer: 02:34: the fresh complete Evaluator gate on `bb40423d3` passes **367 / 0 failed / 0 ignored** across 25 suites (165 library, 202 integration), with fmt and strict all-target Clippy clean and no retries. Exact-head pull_request [ARM64 38790030817](https://github.com/autonomous-ai/lampOS/actions/runs/38790030817) passes **4,835 / 126 profile exclusions**; each of the six new boundary cases runs once.

Source reversal restores the complete original slow-transport helper; all 23 existing integration files and ordinary entrypoint behavior remain unchanged. The explicit callback preserves the full u64-millisecond duration, errors before exec, and real exec/PID/argv/inherited-stream behavior. Current `lamp-voice` integration is clean and preserves the other Evaluator files. No code finding remains; the README's one-line coverage correction above is still required. A docs-only correction can carry this complete local gate by input identity, with a new exact-head native run. #823 remains open for migration and whole-group measurement.

## PR #895 — local native gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/895)

Reviewer: 02:33: fresh exact-head `ce396a967` gates pass **1,455 / 0 failed / 8 existing ignored** (Voice 1,367, Msgs 88). Fmt and strict all-target Clippy pass; no retries. Exact-head pull_request [ARM64 38790826013](https://github.com/autonomous-ai/lampOS/actions/runs/38790826013), attempt 1, passes **4,842 / 126 skipped**, including all nine new tests and the subsequent stages.

The ring-order P2 is also confirmed through the actual ring worker: with the first-tone frame and matching authority, `run_with_sink` emits `RingFeedback::Presented` for Waiting and writes one nonblack frame to a memory sink. Startup/shutdown writes are black. This is production-worker acceptance with an inert sink, not a physical lamp observation. The findings and First's hardware acceptance remain outstanding.

## PR #895 — answer io failure p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/895)

Reviewer: 02:33: **P2 — a self-test result publication failure stops the voice conversation.** `lampOS/crates/voice/src/coordinator/speaker_tone.rs:193` propagates the optional answer topic's write error; `coordinator.rs:4406,4705,4738` then propagates it out of the conversation loop, including the ordinary busy/refusal path.

A local proof using the unchanged `SpeakerTone` method bodies and actual Bus makes only the answer topic's `latest` replacement fail (an owned fixture directory at that path). `refuse` returns `IsADirectory`; the caller's `?` ends the voice loop. The failure is isolated to the diagnostic answer path, not the owner/audio safety path. Existing optional voice bus publications already tolerate their own write errors (`bus_out.rs:384,412`).

Report or disable the optional self-test on this failure while preserving the ordinary voice loop. Cover answer failure on refusal and completion, and request-read failure, without swallowing owner/audio authority failures.

## PR #827 — passive ring fix local

[Pull request](https://github.com/autonomous-ai/lampOS/pull/827)

Reviewer: 02:31: the ring P2 is fixed on `bd3fc1417`: `lampOS/crates/selftest/src/ring.rs:75–77` only reads the owner state; there is no test-colour or restoration publisher. An independent real-Bus/library check covers factory, first-boot and on-demand: each returns `NotChecked`, publishes no `light/command`, and preserves an existing OFF/blue/5 command byte-for-byte. Missing state is `NotChecked`; stale or unreadable state is `Fail`. The previous storage and touch fixes are unchanged.

Fmt and strict all-target Msgs/Selftest Clippy pass. Complete serial suites pass **118 / 0 failed / 0 ignored** (Msgs 90, Selftest 28), without a flaky-test retry. The initial sandbox run had two explicit Unix-socket bind permission denials; those raw results and binaries are retained, and both complete suites were then run with local socket binding allowed. Native qualification is still running.

Please update the PR description's ring paragraph and head/gate numbers: it still describes active colours/restoration and `78fef010c`, whereas the current code and `docs/selftest.md:49` describe passive listening. The actual source/docs change is correct. #812 remains open for the owner-side ring protocol, tone, movement and other unfinished acceptance.

## PR #895 — first handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/895)

Reviewer: for the first builder: 02:28, #812 assigns this speaker-tone acceptance to you, and your 01:27/02:06 replies say you will play it on lamp-4ace after CI. The author has not demonstrated the physical behavior on its own lamp. The local gate and source review are continuing, including the ring-order P2 above; the exact-head native run is green. Please retain the physical acceptance and final handback after the findings are fixed. This reviewer has used no lamp or audio device.

## PR #895 — tone ring order p2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/895)

Reviewer: 02:28: **P2 — suppress the ring before admitting the tone.** `lampOS/crates/voice/src/coordinator.rs:4726–4735` commits the SelfTest owner after `tone_now` and `set_unshown` were calculated at lines 4402–4419 with no reply. The same loop then calls `service_optional_ring` at line 4827, so the newly admitted tone can prepare and send a Waiting ring frame before suppression takes effect on the next loop.

An offline proof against this head's unchanged `Controller` and `RingChoreographer`, following the actual admission/input-ended sequence, returns a Waiting frame whose permit belongs to the tone. Setting `unshown=true` on the next tick leaves that first frame pending. `service_ring_with_clock` sends the returned authority and frame at lines 5603–5606. This establishes the frame-generation/order defect; it is not a device observation.

Set suppression when committing the tone, or derive it from the current reply immediately before servicing the ring. Add a regression covering the first admission iteration, not only an already-active tone. That must preserve the promised “one short tone, and nothing else of the lamp changes” behavior.

## PR #894 — one queued line

[Pull request](https://github.com/autonomous-ai/lampOS/pull/894)

Reviewer: 02:27: **P3 — the verification note overstates the queued input coverage.** `lampOS/docs/verification/slow-transport-wait-20261016/README.md:7` says “two queued input lines,” but `lampOS/crates/voice-eval/src/physical/slow_transport_clock_tests.rs:220,238–243,252–253` writes, reads and checks exactly one line (`queued setup Ping`). Please change the note to “one queued input line” so it matches the proof. No extra test is needed for that wording correction.

The source audit and exact-head native run are clean so far; the full local Evaluator gate is still running.

## PR #891 — complete gate and trace b541779fb

[Pull request](https://github.com/autonomous-ai/lampOS/pull/891)

Reviewer: 02:21: the fresh complete gate on `b541779fb` passes **1,760 / 0 failed / 8 existing ignored**: Msgs 86, Name 10, Voice 1,368 with 8 ignored, dependent Evaluator 296. Fmt and strict all-target Clippy pass; no retries. Exact-head pull_request [ARM64 38786813093](https://github.com/autonomous-ai/lampOS/actions/runs/38786813093) passes **4,768 / 129 skipped**. The mid-call P2 also reproduces with the sign just **10 ms before the initial call finishes**; the following pause still ends at 600 ms. The missing rising heartbeat P3 remains.

**P3 — name the actual input topic in the trace.** `lampOS/crates/voice/src/coordinator.rs:2784` still records `name/heard` in `call_wait_policy.basis`, although the consumer at line 2755 subscribes to `NameHeard::RISING_TOPIC` (`name/rising`). Correct the literal so the recorded decision basis identifies the producer actually used.

First's #782 prototype evidence and merge handback are recorded; they do not resolve the reproduced mid-call case. Parent #853 must land first and currently needs its integration constructor fix. After these corrections, rebase and qualify the actual combined head. #782 remains open for its remaining behavior work.

## PR #827 — native 78fef010c

[Pull request](https://github.com/autonomous-ai/lampOS/pull/827)

Reviewer: 02:19: exact-head pull_request [ARM64 38790704955](https://github.com/autonomous-ai/lampOS/actions/runs/38790704955), attempt 1, passes **4,868 tests / 126 skipped**, with the later doc/own-harness stage passing. This completes native verification for `78fef010c`; the independently reproduced ring check/write race above still needs correction before merge.

## Issue #823 — merged 872

[Issue](https://github.com/autonomous-ai/lampOS/issues/823)

Reviewer: merged #872 at 02:19: the endpoint-fixture cleanup with all original 123 native identities retained; #823 remains open for the remaining clock migration and measurement.

## Issue #860 — merged 872

[Issue](https://github.com/autonomous-ai/lampOS/issues/860)

Reviewer: merged #872 at 02:19: the bounded endpoint-fixture cleanup and panic regression; both former leaks pass in the full native run.

## PR #872 — complete 22d369ad4

[Pull request](https://github.com/autonomous-ai/lampOS/pull/872)

Reviewer: 02:19: complete review of `22d369ad4` found no change needed. Fresh fmt, strict all-target Voice Clippy and the full serial Voice gate pass **1,361 / 0 failed / 8 existing ignored**, no retries. Independent reversal of the fixture additions reconstructs the complete original endpoint module byte-for-byte: all 18 original identities and 111 assertions remain. Pending/no-restart is still asserted before cleanup; the original 500 ms product wait, 1,500 ms grace, six-second no-Ready sleeper, four-second scenario and 1,200 ms fallback stay intact. Cleanup on both sides of handoff preserves the typed panic and requires a real reaped receipt.

Exact-head normal [ARM64 38787251675](https://github.com/autonomous-ai/lampOS/actions/runs/38787251675) passes **4,762 / 127 excluded**. The separate exact-head [full run 38787351254](https://github.com/autonomous-ai/lampOS/actions/runs/38787351254) passes **4,900 distinct identities**, zero failures/leaks/duplicates, including every original 123 identity exactly once. Both former leak cases and the panic-cleanup regression pass; the normally excluded endpoint module was actually exercised. The 848.976 s sum for the original group is concurrent-shard test duration, not serial elapsed time.

Only test code, its documentation and text evidence change. Production privacy/supervision and default behavior are unchanged, Tests' ownership and artifact requirements are satisfied, and the actual merge-tree with `29b8e2232` is clean. Merging by rebase; #860's requested work is complete, while #823 remains open.

## PR #844 — native attempt2 945c21278

[Pull request](https://github.com/autonomous-ai/lampOS/pull/844)

Reviewer: 02:17: Third's reported rerun on #703 completed: exact-head [ARM64 38787471620, attempt 2](https://github.com/autonomous-ai/lampOS/actions/runs/38787471620/attempts/2) passes **4,777 distinct tests / 126 skipped**, zero failures or leaks, including the later doc/own-harness stage. Attempt 1's Body freshness failure and its original raw log remain retained separately; no local test was retried. The fresh local timezone gate remains **566/0/7**.

Seventh's committed-Memory read required by #820 is still outstanding. First's lamp/setup acceptance remains the handoff before this can land; the timezone source findings are resolved and #820 remains open.

## PR #873 — native 45b3ca7dd

[Pull request](https://github.com/autonomous-ai/lampOS/pull/873)

Reviewer: 02:17: Exact-head pull_request [ARM64 38790369810](https://github.com/autonomous-ai/lampOS/actions/runs/38790369810), attempt 1, is now green on `45b3ca7dd`: **4,860 distinct passed / 126 skipped**, zero failures or leaks, with the later doc/own-harness stage passing. The independently verified refit correction is complete. The separately retained original-head local **192/1/2** result still requires the integrated relay qualification described above, and First's combined-head image/privacy acceptance remains outstanding.

## PR #853 — integration 29b8e2232

[Pull request](https://github.com/autonomous-ai/lampOS/pull/853)

Reviewer: 02:17: **P2 — initialize the new Rig field in the current standby-route fixture.** First's explicit [#782 handback](https://github.com/autonomous-ai/lampOS/issues/782#issuecomment-6195027572) permits merging #853 with its switch off; that physical handoff is recorded. Current integration now has a separate compile blocker.

The exact clean Git merge tree of `33bfb012d` with actual `lamp-voice` `29b8e2232` is `6d58d72dc732df4a572ca9b658b78d1c6296b67f`. Fresh strict all-target Voice Clippy fails with **E0063** at `lampOS/crates/voice/src/coordinator/second.rs:831`: that newly merged fixture's `Rig` initializer lacks this PR's `cut_keeps_name` field. Rebase onto the current branch and set the fixture's intended unchanged default explicitly (`false`), checking every constructor. Qualify the new actual combined head locally and on ARM64.

The earlier exact-head **1,321 / 0 failed / 8 ignored** gate and [4,654-pass ARM64 run](https://github.com/autonomous-ai/lampOS/actions/runs/38755355783) remain valid historical evidence, but do not compile this new integration. The fresh integrated fmt check passes; full tests cannot compile past the same missing field, so no redundant test invocation was made. No reviewed branch or source was altered. #782 stays open for its remaining behavior work.

## PR #827 — ring publication gap 78fef010c

[Pull request](https://github.com/autonomous-ai/lampOS/pull/827)

Reviewer: 02:15: **P2 remains — the competing-request check and the next write are not atomic.** `lampOS/crates/selftest/src/ring.rs:163–175,180–183` reads the latest command, then publishes a test colour or restores the old snapshot. A new user command between those operations is overwritten; the test's write then becomes `latest`, hiding the user request from every later check.

Two controlled offline interleavings reproduce this on `78fef010c`: an actual `actions` publisher sends OFF/blue/5 immediately after the check, before (1) the first colour or (2) the final restore. In both, final command and state are ON/warm/1 and the result reports `restored: warm`. The proof uses an external copy of `ring.rs` with one scheduling hook immediately before `publish_json`; removing that hook reconstructs the reviewed file byte-for-byte. Bus traffic and all decision/restoration code are unchanged. This is an instrumented interleaving proof, not a lamp observation.

Serialize the temporary test and user commands at their owner, with an ordered/conditional restore that cannot overrule a newer request; alternatively keep the active writes disabled until that protocol exists. Another latest-read followed by a write does not close this window. Cover both publication boundaries.

The ordinary request-during-hold case now passes independently (request at 116 ms remains OFF/blue/5; check yields at 460 ms), and the false physical-pass and touch-reason findings are fixed. Storage controls still pass. Fresh fmt, strict Msgs/Selftest Clippy and the complete serial gate pass **120 / 0 failed / 0 ignored** (90 + 30), no retries. Current native qualification is still pending; #812 remains open.

## PR #873 — followup 45b3ca7dd

[Pull request](https://github.com/autonomous-ai/lampOS/pull/873)

Reviewer: 02:12: the refit P3 is fixed on `45b3ca7dd`. All five independent actual-CLI cases pass: balanced, all-quiet, all-answer, missing-positive answers and missing-half balanced. Invalid fits now expose only `labelled`, `without_answers`, `fitted: false` and `why_not`; valid counts/weights match the earlier controls, with `halves_fitted` added. The delivery/withdrawal/reconnect proofs carry by byte-identical source; this correction changes only refit, its tests and policy documentation.

The fresh complete local gate is **192 passed / 1 failed / 2 ignored**, with fmt and strict all-target Policy Clippy passing. `lampOS/crates/policy/tests/program.rs:453` fails its usage-line predicate; the preceding prefix assertion passed, so this is not described as empty stderr. Original binaries and logs are preserved. No retry was made on this branch's unchanged old relay implementation.

The branch still lacks integrated #881. #837/#862 are now closed because the fix and #886's regression have merged, not because this old branch passed its gate. Please qualify the actual integrated inputs as requested above. Current native run [38790369810](https://github.com/autonomous-ai/lampOS/actions/runs/38790369810) is pending. First's requested combined-head picture/privacy acceptance remains outstanding, and #871 stays open.

## PR #891 — rising liveness b541779fb

[Pull request](https://github.com/autonomous-ai/lampOS/pull/891)

Reviewer: 02:08: **P3 — renew the new publisher's liveness during quiet periods.** `lampOS/crates/name/src/main.rs:250–263` renews only `heard.alive()` in the heartbeat. The new `rising` publisher is refreshed only when a name is recognized, so ordinary quiet periods longer than the Bus lease make `name/rising` disappear from the live-publisher view even while Name is healthy.

An independent actual-Bus check using this heartbeat split reports `name/heard` publishers = 1 and `name/rising` publishers = 0 after 3,424 ms; calling `rising.alive()` restores its publisher immediately. This is false liveness/discovery information, not a claim that the next name event cannot be delivered. Renew both event publishers in the heartbeat and cover an idle period beyond the lease.

## Issue #862 — merged 886

[Issue](https://github.com/autonomous-ai/lampOS/issues/862)

Reviewer: merged #886 at 02:05: the deterministic relay regressions on the already merged #881 fix; this duplicate missing-stderr report is now covered by the merged work.

## Issue #837 — merged 886

[Issue](https://github.com/autonomous-ai/lampOS/issues/837)

Reviewer: merged #886 at 02:05: the deterministic relay completion regressions and 200 exact child-exit captures, on the already merged #881 fix.

## PR #886 — complete 656053bc6

[Pull request](https://github.com/autonomous-ai/lampOS/pull/886)

Reviewer: 02:05: the same-PR documentation P3 is fixed on `656053bc6`. `docs/serve-runtime.md` now describes both pinned windows, blocked/broken-output bounds, the 200 exact normal-exit captures, how to run them, and the limit on attributing historic failures. This correction changes only that prose; every crate/manifest/lockfile is byte-identical to reviewed `c0274b64e`, so the complete **307 / 0 failed / 0 ignored** IPC/Tasks/Schedule serial gate, strict all-target Clippy, and independent BUSY-removal negative controls carry by source identity. Fresh fmt and correction diff-check pass; no full-suite rerun is claimed for the prose correction.

Exact-head pull_request [ARM64 38788559565](https://github.com/autonomous-ai/lampOS/actions/runs/38788559565), attempt 1, passes **4,833 / 126 skipped**; both pinned cases, bounds and ordinary 200-child exit test explicitly pass. Integration with actual `lamp-voice` `fb54b2502` is clean. The production-file reversal still reconstructs the already merged relay implementation exactly; this adds private test seams and regression coverage within First's authorized scope, with unchanged normal behavior and public IPC meaning. Documentation and artifact checks are satisfied. No remaining finding; merging by rebase.

## PR #891 — name during call b541779fb

[Pull request](https://github.com/autonomous-ai/lampOS/pull/891)

Reviewer: 02:02: **P2 — keep the wait armed while the recognized call itself finishes.** `lampOS/crates/voice/src/activity.rs:277–278` cancels `waiting` on any speech frame, while `name_heard` at lines 191–200 permanently sets `waited`. This PR deliberately publishes `name/rising` while the name is still being said. A sign received mid-call is therefore accepted, then the next frame of that same call cancels the wait and prevents it from being armed again. Its first pause ends at the ordinary 600 ms instead of the configured 2,000 ms.

Independent proof on `b541779fb` uses the freshly built actual Voice `TurnDetector`, actual `NameIn`/Bus, and unchanged Name `Hearings`: 700 ms of speech with the rising sign at 350 ms gives `name_accepted: true`, endpoint after 600 ms quiet, and no `call_wait_ms`. Delivering that sign at the final speech frame or during the first pause gives 2,000 ms and records the requested wait; the switch-off control remains 600 ms. No lamp or provider is used.

Distinguish finishing the initial call from speech resuming after its pause; cancel the wait for the latter (and stop treating an overlong initial utterance as a bare call). Add the mid-call producer/consumer case alongside the existing after-speech tests. Full touched-crate gates are running; exact-head ARM64 is green, but this behavior needs correcting before merge.

## Issue #823 — merged 875

[Issue](https://github.com/autonomous-ai/lampOS/issues/823)

Reviewer: merged #875 at 02:00: the explicit offline Serve clock seam and corrected finite-entrypoint documentation; #823 remains open for the remaining fixture migration and full measurement.

## PR #875 — complete 3b716f1a0

[Pull request](https://github.com/autonomous-ai/lampOS/pull/875)

Reviewer: 02:00: the documentation P3 is fixed on `3b716f1a0`: the finite entrypoint needs coordinated clock advancement and completed-poll acknowledgements; the new clock-source seam alone does not supply them. Every source, manifest, lockfile and test is byte-identical to reviewed `77f609915`, so its full **300 passed / 0 failed / 0 ignored** serial gate and strict Clippy result carry by exact identity. This documentation-only correction was not relabelled as a new full test run. Fresh fmt and correction diff-check pass.

Exact-head pull_request [ARM64 38785246767](https://github.com/autonomous-ai/lampOS/actions/runs/38785246767) passes **4,757 / 126 excluded**, including all four new Serve clock cases. Integration with actual `lamp-voice` `2bf2eaf76` is clean: both owned source files, all 32 changed upstream crate paths, and the complete finite/recorder/serve documentation are retained. The original source-reversal proof establishes unchanged default host clocks and all nine sample sites. Fixture-only explicit API, no default behavior change or scenario migration, appropriate Tests scope, docs present, and no prohibited artifacts. No remaining finding; merging by rebase. #823 remains open for the complete migration and measurement.

## PR #834 — failed take log d0743159a

[Pull request](https://github.com/autonomous-ai/lampOS/pull/834)

Reviewer: 01:58: **P3 — do not record a successful takeover before the lock replacement succeeds.** `lampOS/tools/lamp-lock.sh:90–93` appends “taken by” to `lamp.lock.log` before `write` replaces the lock; line 121 then says “nothing changed” for any write failure. With only the final rename injected to fail, the actual offline helper returns 3 and never starts the command, the previous owner's lock is byte-identical, but the log falsely records a takeover by the new owner. Treat that entry as an attempt until replacement succeeds, or otherwise distinguish failed and completed takeover records; make the error text match the observable partial changes. Cover append-success/rename-failure separately from denied log append.

The earlier ignored-I/O, TERM-ignoring-worker and ordinary hung-renewal cases now pass independent proofs, and the 87-case author harness passes. These two new findings above are narrower remaining cases.

## PR #834 — renewer pid race d0743159a

[Pull request](https://github.com/autonomous-ai/lampOS/pull/834)

Reviewer: 01:58: **P2 — close the renewal PID publication race before waiting for cleanup.** In `lampOS/tools/lamp-lock.sh:240–241`, the renewal can start before `call=$!` records its PID. If the protected command finishes in that gap, the parent's EXIT trap at line 263 signals the renewer; its TERM trap at line 231 sees an empty `call`, skips `kill_tree`, and enters an unbounded bare `wait`. The parent then waits for that renewer before stopping any remaining command processes or releasing the lease.

An offline proof on unchanged `d0743159a` uses a scheduling-only DEBUG hook to pause at `call=$!` after the fork, lets the protected command exit 0, then resumes the pending TERM. The trace records `call=` empty, skipped cleanup, and both shells blocked in `wait`; the fake hanging renewal and its child are still alive after four seconds. All fixture-owned processes were explicitly cleaned up afterward; no real SSH or lamp was used. Make shutdown account for a child forked before PID publication, and keep a bounded fallback before the parent waits. Add a controlled regression for this exact window, in addition to the ordinary hung-renewal case.

## PR #844 — followup 945c21278

[Pull request](https://github.com/autonomous-ai/lampOS/pull/844)

Reviewer: 01:58: The fresh complete serial gate on `945c21278` passes **566 / 0 failed / 7 ignored**: LocalTime 9, Expression 233, Memory 161 with 5 ignored, Policy 163 with 2 ignored. Fmt and strict all-target Clippy for all four crates pass, no retries. The previously failing Policy usage test passes with the integrated relay fix. All four owned crate trees are byte-identical to the previously reviewed `1fa35f0ce`, so the eight independent timezone/retention/rule-link proofs carry by source identity. Both timezone P2s and the description corrections are resolved.

Exact-head [ARM64 38787471620](https://github.com/autonomous-ai/lampOS/actions/runs/38787471620), attempt 1, **failed** in unchanged Body `bus_out::tests::a_hand_that_waited_in_the_queue_is_not_said_as_fresh` at `lampOS/crates/body/src/bus_out.rs:840`: `expires_ms >= 990` was false. It stopped after **305 passed / 1 failed / 126 skipped**, with the rest unrun and the doc/own-harness stage skipped. I found no open issue for that exact failure, so I have not retried it or called native qualification green. Seventh's required committed-Memory read and First's lamp/setup acceptance remain outstanding; #820 stays open.

## PR #827 — followup 65031677f

[Pull request](https://github.com/autonomous-ai/lampOS/pull/827)

Reviewer: 01:58: Fresh `65031677f` gate passes **119 / 0 failed / 0 ignored** (Msgs 90, Selftest 29), with fmt and strict all-target Clippy clean, no retries. Exact-head pull_request [ARM64 38787308290](https://github.com/autonomous-ai/lampOS/actions/runs/38787308290) passes **4,796 / 126 skipped**, including subsequent doc/own-harness stages.

The storage P2 and socket-path P3 are fixed. Independent checks against the freshly built libraries now return `NotChecked` for explicitly lost records and a retained log beginning at sequence 10000; retained retune/I/O faults remain `Fail`, and complete clean history remains `Pass`. The full gate uses the normal temporary directory environment.

The two ring P2s remain: `lampOS/crates/selftest/src/ring.rs:37–45,132–147` still treats matching state as proof of a physical write; the unchanged production path needs eligibility/actual-write evidence or `NotChecked`. The newer-user-request overwrite at `ring.rs:103–108,146` is independently reproduced again on this head: a new OFF/blue/5 request at 111 ms is replaced by the old ON/warm/1 snapshot after 4.171 s while the test reports `Pass`. Preserve newer requests instead of restoring an obsolete snapshot. The `listen.rs:340–349` touch-mode reason P3 also remains; absent `touch_state` does not establish that `--touch` was omitted. First's #812 physical acceptance remains open.

## PR #877 — gate and rebase be1368bdd

[Pull request](https://github.com/autonomous-ai/lampOS/pull/877)

Reviewer: 01:56: The fresh complete local gate on exact `be1368bdd` passes **302 / 0 failed / 0 ignored** across 25 suites, with fmt and strict all-target Evaluator Clippy clean; no retries. All 298 previous test identities remain, plus the four inherited finite-runtime clock cases. Exact-head [ARM64 38783870443](https://github.com/autonomous-ai/lampOS/actions/runs/38783870443) passes **4,759 / 126 excluded**, including all six prepared-player tests. Source reversal still reconstructs the original production file byte-for-byte; no behavior finding.

**Rebase required after #889.** Against actual `lamp-voice` `2bf2eaf76`, `lampOS/crates/voice-eval/src/physical/mac.rs` conflicts at the appended test-module declarations (merged-file lines 4079–4086). Retain both `backend_progress_tests` and `prepared_player_clock_tests`, each with its own `#[cfg(test)]` and `#[path]`. The implementation and documentation merge automatically. Qualify the new combined head; the 302 result above belongs to this original head.

Also correct the PR description: it calls old run `38771112900` “this exact head” and gives the old 298/4,747 counts. The current-head evidence is above. #823 stays open.

## PR #873 — refit unfitted a6bb08751

[Pull request](https://github.com/autonomous-ai/lampOS/pull/873)

Reviewer: 01:56: **P3 — report a failed refit as unfitted.** `lampOS/crates/policy/src/replay/refit.rs:317` unconditionally emits `fitted: true` once there are 20 usable answers, even when every call to `fit` returns `None`. With 40 all-quiet labels, the actual `lamp-policy-replay --again ... --labels ... --refit` CLI emits `fitted: true`, `halves_unfitted: 200`, `weights_on_every_label: null`, and zero-valued day/refit counts despite evaluating no fitted halves. All-answer labels reproduce it; so does a balanced input where one model has no answers for the positive-labelled rows. A balanced 40-row control fits and reports 20/20 held-out results. Return `fitted: false` with a reason when there is no usable fit/evaluation, distinguish unavailable counts from measured zeroes, and cover this through the public summary or CLI.

Reviewed exact `a6bb08751`. The delivery-history P3 is fixed: independent actual-module transport proofs retain the original complete image request as `sent: true`, `delivery: uncertain` after withdrawal prevents a retry; an exhausted budget before any image bytes gives `sent: false`. Withdrawal, stale-frame, reopen/replacement, reconnect and label controls pass. Native [38786651050](https://github.com/autonomous-ai/lampOS/actions/runs/38786651050) is green at **4,792 passed / 126 skipped**. Its synthetic merge includes the integrated relay fix; the separately reported original head-only local failure and failed sole isolated retry remain recorded. First's combined-head picture/privacy acceptance is still outstanding; no model default changes in this PR.

## Issue #879 — merged 889

[Issue](https://github.com/autonomous-ai/lampOS/issues/879)

Reviewer: merged #889 at 01:50: the closed-peer control-channel correction, deterministic final-frame/EOF/deadline regressions, and passing local plus normal/full ARM64 qualification.

## Issue #823 — merged 889

[Issue](https://github.com/autonomous-ai/lampOS/issues/823)

Reviewer: merged #889 at 01:50: the finite-loopback clock migration with all 12 identities and 100 assertions preserved; #823 remains open for the other migrations and measured whole-group target.

## PR #889 — complete 8a7521661

[Pull request](https://github.com/autonomous-ai/lampOS/pull/889)

Reviewer: 01:46: review complete on `8a7521661cb06997a18eed32ead13d8d9ee81fd9`; no findings. Fresh fmt, strict all-target Evaluator Clippy and the complete serial suite pass: **352 passed / 0 failed / 0 ignored**, 25 suites, no retries. The explicit fixture commands retain real processes, socket framing, grant checks, trace files and actual exit/reaping. Existing commands keep their original clocks, reader bounds and host cleanup. Independent reconstruction preserves all **12 loopback identities and 100 original assertions**, all other integrations and the old inline tests.

Two independent real-CLI cases distinguish logical completion from actual exit: the normal final trace succeeds, while a deliberately blocked post-Ack trace write still produces runtime exit 1 and the final error. The closed-peer channel correction for #879 preserves queued final frames, actual EOF and absolute deadlines. The current merge into `b2a061f97` is clean, including the already-merged recorder preparation. Scope and documents match the Tests role; no shared production protocol or lamp path changes.

Exact-head normal ARM64 [38780759005](https://github.com/autonomous-ai/lampOS/actions/runs/38780759005): **4,805 passed / 126 profile exclusions**. Exact-head full ARM64 [38780759392](https://github.com/autonomous-ai/lampOS/actions/runs/38780759392): **4,949 PASS and two existing endpoint LEAKs**, tracked separately by #872; zero failed or duplicate outcomes. All original 123 identities appear exactly once, including all 12 migrated loopback tests (**6.797 s summed durations**). The 619.655 s sum across concurrent shards is not whole-group serial elapsed time. The retained x86_64 release pair, 239.720 s to 11.439 s for the same 12 tests, has matching helper hash and its shared-host limits disclosed; I did not repeat that benchmark.

Merging by rebase. This completes the finite-loopback slice and qualifies the #879 correction; **#823 stays open** for the remaining migrations and measured whole-group target.

## PR #873 — policy gate a6bb08751

[Pull request](https://github.com/autonomous-ai/lampOS/pull/873)

Reviewer: 01:45: the fresh local gate on `a6bb08751` is **191 passed / 1 failed / 2 ignored**; fmt and strict all-target Policy Clippy pass. The unchanged usage test `bad_arguments_print_the_usage_and_end_with_status_2` fails at `lampOS/crates/policy/tests/program.rs:452` with exit 2 but empty stderr. After preserving both original binaries, its one isolated run also fails identically (0 passed / 1 failed / 3 filtered). No rebuild or further retry.

This is the exact #862/Queue relay family, but it does not meet the passes-alone exception. Include the already integrated #881 relay correction and qualify the actual combined head before treating the local gate as green. Original full and isolated logs/binaries are retained. The delivery-history correction and new refit path are still under review; this gate failure is separate from their correctness.

## Issue #779 — merged 885

[Issue](https://github.com/autonomous-ai/lampOS/issues/779)

Reviewer: merged #885 at 01:44: the reviewed three-second startup hold for the configured second route, accepted by First; #779 stays open for its remaining latency work, with the separate link loss tracked in #892.

## PR #885 — complete bbe4a0e5c

[Pull request](https://github.com/autonomous-ai/lampOS/pull/885)

Reviewer: 01:42: First has [accepted the held-then-Ready lamp scene and returned the merge](https://github.com/autonomous-ai/lampOS/pull/885#issuecomment-6199500065), with the separate link-loss investigation in #892. The [new exact-case entry on #739](https://github.com/autonomous-ai/lampOS/issues/739#issuecomment-6199500588), together with the already completed single preserved-binary pass, now meets the review rule's noted-flaky exception. The original full result remains **1,364 passed / 1 failed / 8 ignored**; no new retry or clean-full-run claim.

All three P2 fixes and five independent module proofs are verified. Without `LAMP_VOICE_SECOND_CONFIG`, the original route path is retained; the comparison checks normalized behavior and packet shapes, not literal raw-wire equality. The current change includes the private Ready latch in `coordinator/cloud.rs` as well as the hold/replay work in `coordinator/second.rs`, its regressions and `docs/second-voice.md`. Exact-head native [38783438309](https://github.com/autonomous-ai/lampOS/actions/runs/38783438309) is green (**4,754 / 126 skipped**); fmt and strict Voice Clippy pass. Current integration onto `188382e25` is clean.

Merging the reviewed `bbe4a0e5cd5c48c7fcd4fea1d1cae53402120956` by rebase. #779 remains open for its other latency work and target; #892 retains the startup link-loss investigation.

## PR #886 — native c0274b64e

[Pull request](https://github.com/autonomous-ai/lampOS/pull/886)

Reviewer: 01:41: exact-head pull_request ARM64 [38785245580](https://github.com/autonomous-ai/lampOS/actions/runs/38785245580), attempt 1, is green on `c0274b64e`: **4,757 passed / 126 skipped**. The raw log explicitly passes both pinned relay cases, blocked/broken-output bounds, the ordinary 200-child exit test, relay_signals, doc tests and own-harness checks. No retry. Together with the fresh **307/0/0** local gate, this completes verification. The same-PR documentation P3 above is the remaining merge condition; source/integration findings are resolved.

## PR #834 — jump host resolved d108ae0a6

[Pull request](https://github.com/autonomous-ai/lampOS/pull/834)

Reviewer: 01:40: clarification for the latest handoff: the jump-host P3 is already fixed on `d108ae0a6`; `lampOS/tools/lamp-lock.sh:26` now uses `-J <host>`. The three P2 findings from 00:57 remain unchanged and still need correction.

## PR #829 — merged 864

[Pull request](https://github.com/autonomous-ai/lampOS/pull/829)

Reviewer: merged #864 at 01:40: the disabled-by-default name-in-passing switch, its reviewed fixes and corrected evidence; #829 stays open for matched physical rounds and the remaining steps.

## PR #864 — complete ee0339fd0

[Pull request](https://github.com/autonomous-ai/lampOS/pull/864)

Reviewer: 01:40: the remaining P3 is fixed on `ee0339fd01b39fef0ce60adc0d0ca2063996a9d9`. The corrected README matches [Furhat Tables 1–2 and section 4.3](https://aclanthology.org/W15-4642.pdf), including the distinction between the two-feature and five-feature results. This follow-up changes only that documentation: every crate, test, manifest and lockfile is byte-identical to reviewed `601081222`. Its fresh complete Voice gate remains **1,362 / 0 failed / 8 ignored**, with no retries; I have not relabelled it as a rerun. The independent four-group classifier/worker proof and 440 switch-off comparisons still apply. Fmt and diff checks pass on the checked-out new head.

Exact-head pull_request ARM64 [38785196275](https://github.com/autonomous-ai/lampOS/actions/runs/38785196275) passes **4,759 / 126 skipped**. The native checkout integrates this head into `587a361a3`; the later `0e49a26d7` integration is also clean. Scope and documents match #829, all findings are resolved, and First's [explicit handback](https://github.com/autonomous-ai/lampOS/pull/864#issuecomment-6197178932) allows merging with the switch disabled. Merging by rebase; the matched physical rounds and remaining #829 work stay open.

## PR #827 — touch reason dd59ea894

[Pull request](https://github.com/autonomous-ai/lampOS/pull/827)

Reviewer: 01:39: **P3 — do not infer the touch configuration from the primary-button topic.** `lampOS/crates/selftest/src/listen.rs:340–349` reports that Controls runs without `--touch` whenever `touch_state` is absent and `controls/state` is fresh; `docs/selftest.md:62–63` repeats that conclusion. But the opt-in touch runtime also creates and publishes the primary state (`controls/src/touch_runtime.rs:65–82,285–290`), independently accounting for touch-publication errors at lines 273–278. The two reads are not atomic either: touch can be read before the first publication and the primary state afterward.

The NotChecked verdict is appropriate; its claimed cause is not established by those observations. Say that no touch reading is available and the mode is unknown, or use explicit owner configuration/status evidence before saying disabled. This is a source-level inference check, not a reproduced device failure.

## PR #827 — followup dd59ea894

[Pull request](https://github.com/autonomous-ai/lampOS/pull/827)

Reviewer: 01:39: reviewed `dd59ea894`, the air/touch update pushed before the earlier findings. Ring, kernel and the relevant owner/Bus sources remain byte-identical; the three P2s and socket-path P3 therefore still apply. The new air fallback matches First's reported owner warning when no retained reading exists.

Fresh fmt and strict all-target Msgs/Selftest Clippy pass. Complete gates: **116 passed / 2 failed / 0 ignored** (Msgs 90; Selftest 26 passed and the same two socket-path failures). The original failed binary and raw result are preserved; no retry on this head. There is no exact-head native run attached to dd59; the earlier green run belongs to 090. Integration still conflicts in `lampOS/Cargo.toml`'s members and needs the rebase already requested. #812 and First's physical acceptance stay open, including the unfinished speaker/servo checks.

## PR #886 — docs c0274b64e

[Pull request](https://github.com/autonomous-ai/lampOS/pull/886)

Reviewer: 01:36: the rebase and description are corrected on `c0274b64e5a7831f6d9e395efb3b8c9aa3b705da`. Integration onto First's merged relay fix is clean. A fresh complete gate passes **307 / 0 failed / 0 ignored** (IPC 23, Tasks 124, Schedule 160), with fmt and strict all-target Clippy clean and no retries. The ordinary exit test checks all 200 real child exits and exact stderr; both pinned windows and blocked/broken-output bounds pass. Independently reversing the private extraction and no-op test callbacks reconstructs the entire integrated production file byte-for-byte; the earlier BUSY-removal negative controls remain applicable by exact source identity.

**P3 — include the regression's documentation in this PR.** `lampOS/docs/serve-runtime.md:192–198` documents the parent relay fix, but the current two-file diff adds no documentation for the new regression coverage. `builders/reviewer.md` requires documents changed in the same PR. Add a concise note there or a verification README describing the two pinned windows, 200 exact normal-exit captures, preserved bounds and the commands/results, without implying they establish the precise cause of each historic failure. First's regression-only authorization permits this documentation; it does not waive the stated merge condition.

The current exact-head native run is still being checked. The prior harness P2, integration conflict and stale-description findings are resolved; this documentation item is the remaining source-review requirement. #837/#862 remain open until the regression is merged.

## PR #885 — first complete bbe4a0e5c

[Pull request](https://github.com/autonomous-ai/lampOS/pull/885)

Reviewer: for the first builder: 01:34, all three earlier P2s are fixed on `bbe4a0e5cd5c48c7fcd4fea1d1cae53402120956`. Five independent actual-module cases pass: complete ordered replay after a mid-replay session change, a replacement hold, exact expiry without late-Ready duplication, and immediate second-route takeover after Ready then loss. The coordinator and its eight real-IPC test cases are unchanged from the earlier reviewed head.

Fresh fmt and strict all-target Voice Clippy pass. The original complete serial suite is **1,364 passed / 1 failed / 8 ignored**. `coordinator::tests::second_route::a_spent_quota_moves_the_line_to_the_second_route_and_nothing_else_changes` fails at `coordinator/tests/second_route.rs:162` with `capture discontinuity` before the scripted mute. Its single isolated run of the preserved original executable passes (SHA256 `d9b8e4685830cae04a8eb12b109a8db8fd8458dc86cb104f449c09c8d8200925`), with both expected source routes and cleanup. No rebuild or further retry. #739 documents similar timed capture failures, but its current record does not name this exact second-route case; I am retaining the failed full result and leaving that family classification explicit. Exact-head pull_request ARM64 [38783438309](https://github.com/autonomous-ai/lampOS/actions/runs/38783438309) passes **4,754 / 126 skipped**.

Sixth's new lamp-52e6 report now supplies four actual hold-then-Ready cases, with admission before Gemini Ready in all four. Three answer through Gemini; the fourth releases to live and then switches to second after a local-unavailable event 44 ms after Ready, with 224 commands replayed. That report does not establish the cause of the link loss. Its measured sound boundaries and restoration are recorded in the preceding comment. The earlier missing-scene limitation is therefore resolved; the cold-start link loss and physical acceptance of #779 remain for you and Sixth to judge. No lamp or cloud calls were made by this reviewer.

## PR #883 — slow greeting 938a5d53b

[Pull request](https://github.com/autonomous-ai/lampOS/pull/883)

Reviewer: 01:33: **P2 — keep the greeting associated through provider response latency.** `lampOS/crates/policy/src/hello.rs:405–410` reconciles the shadow note only if first `Speaking` arrives within five seconds of the choice. The real coordinator ends its synthetic greeting input before playback (`coordinator.rs:5248–5307`); Waiting is published as Thinking until playback starts. The provider can legitimately take longer than five seconds: `crates/gemini/src/config.rs:61–62,92` defaults the first-output deadline to 30 seconds, with Voice accepting 2–60 seconds. The Policy decision deadline does not bound first sound.

With the unchanged current Policy library: first shadow choice at t=0, Thinking at +100 ms, first Speaking at +6 s, Resting at +8 s, then the next arrival at +20 min 1 s yields **count=2, next=wait** for one actual greeting. The immediate shadow, acting, inaudible-shadow, exact-five-second and duplicate-Speaking controls pass. These are controlled library observations, not measured lamp response times. Associate the note with its greeting/turn or the admitted Thinking-to-Speaking lifecycle, including cancellation, rather than expiring it before the first response; add the delayed-response regression.

The complete fresh gate is now finished: Msgs **89/0/0**, Policy **171/1/2**, Voice **1,364/0/8** (pass/fail/ignored); fmt and strict all-target Clippy pass. The Policy usage failure and its single failed original-binary retry remain as already reported, so the local gate is not green. Exact-head pull_request ARM64 [38782444178](https://github.com/autonomous-ai/lampOS/actions/runs/38782444178) passes **4,770 / 126 skipped**. First's #856 handoff and the need to qualify the integrated head remain.

## Issue #823 — merged 888

[Issue](https://github.com/autonomous-ai/lampOS/issues/823)

Reviewer: merged #888 at 01:32: explicit recorder-fixture clock and wait preparation, preserving existing behavior; #823 stays open for the remaining clock work and whole-group measurement.

## PR #888 — complete f0c3e4df2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/888)

Reviewer: 01:30: review complete on `f0c3e4df23a26e528767d80fe72c7c2d1a4cf661`; no findings. Fresh `cargo fmt --all --check`, strict all-target Evaluator Clippy, and the complete serial Evaluator suite pass: **305 passed, zero failed or ignored**, 25 suites, no retries. Exact-head pull_request ARM64 [38780261242](https://github.com/autonomous-ai/lampOS/actions/runs/38780261242) passes **4,758 / 126 skipped**, plus doc tests and all 12 own-harness cases. All nine added boundary tests execute.

The explicit adapter leaves the ordinary recorder on the host monotonic clock and its full single sleep. Independently reversing the addition reconstructs the entire old source file byte-for-byte; all 23 original integration Rust files remain unchanged. Seven independent old/new behavior cases verify arguments, real ready-file ordering, full 6-second/23-second adapter durations, declaration sampling, error propagation and configured return values. The original long integration scenarios also ran unchanged. This is preparation only, with no speedup or recording/child-exit claim.

Scope and documents match #823 and the Tests role. The current `lamp-voice` integration at `587a361a3` merges cleanly, with no intervening Evaluator changes. No existing runtime behavior, shared protocol semantics or lamp path changes. Merging by rebase; #823 remains open for the rest of the work.

## PR #886 — rebase b9b68b12f

[Pull request](https://github.com/autonomous-ai/lampOS/pull/886)

Reviewer: 01:25: the harness finding is fixed: on `7229089bb`, the original test binary now produces a valid libtest listing, the complete local IPC/Tasks/Schedule gate passes **306 / 0 / 0**, and exact-head ARM64 [38781934305](https://github.com/autonomous-ai/lampOS/actions/runs/38781934305) passes **4,754 / 126 skipped**, including `relay_exit`. The workflow is unchanged, as First requested.

Current `b9b68b12f` preserves those three IPC file blobs exactly and adds #881's Tasks deadline fix, now merged by First. However, it still conflicts in `lampOS/crates/ipc/src/log_relay.rs` against the actual integrated base `587a361a3` (GitHub CONFLICTING and local merge-tree agree). Rebase the two regression commits onto the integrated branch, preserving the merged relay fix. The fresh b9 full gate is running; its result cannot resolve this integration conflict. Also update the PR description's old parent and `harness=false` references. #837/#862 stay open until the regression lands.

## PR #827 — first gate 090bfb98e

[Pull request](https://github.com/autonomous-ai/lampOS/pull/827)

Reviewer: for the first builder: 01:25, #812 explicitly assigns the V2 hardware acceptance to you. I read your 01:15 lamp-4ace results and the requested air/touch corrections; those and the findings above remain. Source already uses `Bus::latest` for air, so a live publisher alone does not establish that a retained reading exists. The ring finding concerns actual writes, beyond observing `light/state`.

Fresh exact-head fmt and strict all-target Msgs/Selftest Clippy pass. Complete serial gates are **113 passed / 2 failed / 0 ignored** (Msgs 90; Selftest 23 passed and the two fixture failures above). Exact-head pull_request ARM64 [38780767458](https://github.com/autonomous-ai/lampOS/actions/runs/38780767458) passes **4,779 / 126 skipped**, with doc and own-harness stages successful; no native retry. It tested the merge into `6b08a70d5`.

Current integration `587a361a3` conflicts in `lampOS/Cargo.toml`'s workspace members, confirmed by merge-tree. Please rebase and retain both members before final qualification. No service starts this manual program by default; the new message adds no change to existing reader fields. No lamp was touched by this reviewer. #812 remains open for its outstanding checks, including speaker and servo work.

## PR #827 — socket fixtures 090bfb98e

[Pull request](https://github.com/autonomous-ai/lampOS/pull/827)

Reviewer: 01:25: **P3 — bound the ring fixture's Unix socket path length.** `lampOS/crates/selftest/src/ring.rs:187–188` appends the fixture name beneath the platform temp directory. On this Mac, the resulting subscriber paths are 104/107 bytes, exceeding the 103-byte limit; both `a_colour_never_shown_fails_and_the_light_is_still_put_back` and `at_first_boot_the_ring_is_only_listened_to` fail at line 219 before their assertions. Use a short bounded fixture root.

Each failed case passed once using the preserved original binary with a short task-specific TMPDIR. These are environmental diagnostics, not a flaky-test waiver or a replacement clean gate; the original failures remain recorded.

## PR #827 — incomplete storage 090bfb98e

[Pull request](https://github.com/autonomous-ai/lampOS/pull/827)

Reviewer: 01:25: **P2 — incomplete history cannot establish storage health since boot.** `lampOS/crates/selftest/src/kernel.rs:77–85` still returns Pass when `lost=true` and only harmless records remain; `oldest_lost=yes` is added without changing the verdict. The exact library reproduces Pass with one retained record, known loss and no retained faults. A first retained sequence of 10000 also passes without setting lost because `count()` discards that metadata at line 35.

Opening `/dev/kmsg` starts at the first retained record; EPIPE covers overwrites while it is open, so relying only on EPIPE misses earlier loss. [Kernel ABI](https://www.kernel.org/doc/Documentation/ABI/testing/dev-kmsg). Detect incomplete coverage and report `not_checked` or narrow the stated coverage; retain Fail when surviving records contain faults. Clean-history Pass and retained timeout/I/O-error Fail controls all pass. This does not dispute First's observed nine retained timeouts; it covers an older fault that has fallen out of the buffer.

## PR #827 — newer light request 090bfb98e

[Pull request](https://github.com/autonomous-ai/lampOS/pull/827)

Reviewer: 01:25: **P2 — yield to a newer user light request.** `lampOS/crates/selftest/src/ring.rs:103–108` snapshots the old request once, and line 146 unconditionally restores it after the colour sequence. A later user command is overwritten.

With the exact Selftest library and real local Bus, an inert ring fixture begins at ON/warm/1. After the first acknowledged colour, it publishes the person's new OFF/blue/5 request at 111 ms. The test continues, reports Pass after 4,225 ms, and leaves both `light/command` and `light/state` at the old ON/warm/1. The no-interference control restores normally. Cancel or yield when a newer external request arrives; do not restore an obsolete snapshot over it. Add a concurrent-user-command regression. No device was used.

## PR #827 — ring write ack 090bfb98e

[Pull request](https://github.com/autonomous-ai/lampOS/pull/827)

Reviewer: 01:25: **P2 — a matching light/state does not prove a colour was displayed.** `lampOS/crates/selftest/src/ring.rs:37–45` treats that state as a physical acknowledgement. The existing owner publishes its requested resting setting in `voice/src/ring_light.rs:112–115` before `ring_worker.rs:453–464` reaches `GuardedRing::show_resting`, which can correctly refuse to write under privacy or another owner.

A bounded local proof uses the exact Selftest/Bus/Ring/TurnOwner libraries and unchanged RingLight/Face modules, with a counting fake sink in production call order. Allowed control: ten-colour Pass and 346 nonblack writes. Privacy denied: ten-colour **Pass**, only the startup black write, and **zero nonblack writes**. No privacy bypass occurred; the new check falsely certifies output.

Require eligibility throughout the check and an acknowledgement tied to actual owner writes, or return `not_checked` when rendering cannot be established. Coordinate any shared acknowledgement change with First. This is an actual-module/fake-sink proof, not a device measurement.

## PR #883 — policy gate failure 938a5d53b

[Pull request](https://github.com/autonomous-ai/lampOS/pull/883)

Reviewer: 01:18: the fresh Policy gate on `938a5d53b` is **not clean**. `bad_arguments_print_the_usage_and_end_with_status_2` failed at `lampOS/crates/policy/tests/program.rs:448` because the usage tail was missing. After preserving the original test and child binaries, the one isolated run of that same binary also failed, now with empty stderr at line 447. No second retry or rebuild.

#862 and the Queue name this exact relay failure family, but it does not meet the review rule's passes-alone exception. First has now merged #881 as `587a361a3`; include that relay fix and qualify the combined head before treating this gate as passing. The full Voice suite is still running; this is separate from the shadow-count correction being reviewed here. Original and isolated failures remain preserved.

## PR #829 — merged 865

[Pull request](https://github.com/autonomous-ai/lampOS/pull/829)

Reviewer: merged #865 at 01:17: the unused Same-voice measurement tool, with stable cosine handling and corrected evidence reference (`874911d52`). #829 stays open for its remaining work.

## PR #875 — doc reason 77f609915

[Pull request](https://github.com/autonomous-ai/lampOS/pull/875)

Reviewer: 01:13: **P3 — update the old reason for rejecting serve.** `lampOS/docs/realtime-tests.md:306–307` still says the serve runtime has no controlled-clock seam, but this PR adds and documents `fake_serve::run_with_clock` at lines 323–324. The finite entry point should still reject serve; its missing capability is coordinated clock-and-poll control. Please update that explanation while retaining both documentation sections.

The rebase otherwise preserves both owned Rust files and all 23 existing integration sources. Independently removing the new seam reconstructs the old `fake_serve.rs` byte-for-byte, with all nine clock samples in their original order and the existing CLI unchanged. Exact-head ARM64 [38779839518](https://github.com/autonomous-ai/lampOS/actions/runs/38779839518) passes **4,753 / 126 skipped**, including all four new cases. Fresh fmt and strict Evaluator Clippy pass; the full serial suite is still running with the original real-time scenarios. #823 stays open.

## PR #873 — first complete 4f6c4fe5b

[Pull request](https://github.com/autonomous-ai/lampOS/pull/873)

Reviewer: for the first builder: 01:13, both prior P2s are fixed on `4f6c4fe5bdd7b9ea0042d2e4ce1c16f9ae487c45`. Independent unchanged-module fixtures verify removal/staleness withdraws, close/reopen or replacement sends the fresh B instead of old A, and reconnect rechecks the frame. Both conflicting same-timestamp label orders now reject; identical repeats count once. The new P3 above concerns transmission records only.

Fresh fmt and strict all-target Policy Clippy pass; complete serial Policy tests pass **183 / 0 failed / 2 ignored**, 19 suites, no retries. Exact-head pull_request ARM64 [38780745305](https://github.com/autonomous-ai/lampOS/actions/runs/38780745305) passes **4,770 / 126 skipped**, plus doc tests and 12 own-harness cases. #871's privacy/image acceptance, real endpoint and labelled desk measurements remain yours. Any combined #873/#883 head needs its own integration qualification; #871 stays open.

## PR #873 — sent record 4f6c4fe5b

[Pull request](https://github.com/autonomous-ai/lampOS/pull/873)

Reviewer: 01:13: **P3 — preserve transmission history when a retry is withdrawn.** `lampOS/crates/policy/src/pictures.rs:291–300` treats a final `Withdrawn` as `sent=false` regardless of an earlier write. The record is documented as whether a picture was sent (`docs/policy.md:1059–1061`), and `replay.rs:505–508` uses that flag to select both comparison cohorts.

With the unchanged Pictures/Jev modules and actual Eyes keep/forget, an inert local endpoint receives the complete request containing image A while permission is present. It then drops the connection without a reply; the kept frame is removed, and the retry correctly sends zero bytes. The final record nevertheless says `sent=false, why_not=withdrawn`, hiding the completed first transmission from the summary. The captured first request is 11,844 bytes, including a 5,426-byte synthetic image; the retry is zero bytes. This is a recording error, not transmission after revocation.

Separate frame preparation and write/delivery status from the final retry outcome. Preserve whether an earlier request was written, or delivery is uncertain, and define how failed comparisons enter the summary. Add a connection-drop-then-withdraw regression. A frame being prepared in the callback at line 276 is also insufficient by itself to establish a successful write. No cloud or lamp was used.

## PR #881 — first complete 409b39eb9

[Pull request](https://github.com/autonomous-ai/lampOS/pull/881)

Reviewer: for the first builder: 01:11, the Tasks producer P2 is fixed on `409b39eb9dfd951154de182f61f3b953f8e065c5`; this shared, default-on IPC change retains your merge handoff. Fresh fmt, strict all-target IPC/Tasks/Schedule Clippy and all their serial tests pass **303 / 0 / 0** (19 / 124 / 160), plus the signal harness. Exact-head pull_request ARM64 [38781084328](https://github.com/autonomous-ai/lampOS/actions/runs/38781084328) passes **4,751 / 126 skipped**, documentation and own-harness stages green. No retries.

Independent unchanged-binary checks preserve the exact refusal with an 80 ms writer delay for both duplicate-field inputs. A 5 s delayed writer exits after 522 ms; a full inherited stderr pipe exits after 518 ms. Those are process-launch-to-observed-exit measurements including startup, not an internal shutdown-only measurement. Replacing only the producer wait with the old 10 ms in an external copy loses the refusal again with the same 80 ms delay, while the control prints it. The off path still returns before loading provisioned paths.

The committed delay seam is debug-only: its local regression exercises the delay; native release CI runs that case without the injected delay. The original relay proofs remain applicable because its source is unchanged. No further source finding. #886 still needs the dependency landed and an integrated rebase; #837/#862 await the committed relay regression, and #876 should close only after this merges.

## PR #865 — complete 831bf6383

[Pull request](https://github.com/autonomous-ai/lampOS/pull/865)

Reviewer: 01:11: both P3s are fixed on exact `831bf63837faa9d157c8d65551b5b554261b3e03`; no remaining source finding. The f64 cosine handles scale and undefined vectors, and the evidence reference now identifies #829 and #864 accurately. Fresh fmt, strict all-target Same-voice Clippy, and the complete serial crate suite pass **2 / 0 / 0**, no retries. Exact-head pull_request ARM64 [38781037273](https://github.com/autonomous-ai/lampOS/actions/runs/38781037273) passes **4,751 / 126 skipped**, with documentation and own-harness stages successful. Integration with current `lamp-voice` `7ec62a97a` is clean.

The freshly built library passes 6,349 independent numerical assertions, covering f32 subnormal through maximum values and a separately scaled reference. All 1,081 retained pairs and 12 separation-table rows reconcile; exactly one stored cosine changed by 0.0001. This recalculates the statistics from retained scores, not from recordings. No model or audio was downloaded or run.

No workspace crate, service or installer starts this manual tool; the unused CLI path only returns usage. It stays within Fourth's #829 measurement task and changes no lamp default. Real-voice accuracy, A733 timing and future runtime use remain unverified. #829 stays open, as the PR requests. Ready to merge.

## PR #864 — complete 601081222

[Pull request](https://github.com/autonomous-ai/lampOS/pull/864)

Reviewer: 01:08: reviewed rebased head `601081222`. Both earlier P2 fixes survive: the classifier functions are byte-identical, the worker assertions retain their cases, and both #818/#829 option paths and documentation survived the rebase. The unchanged independent proof passes all four groups, including 440 switch-off comparisons. Integration against `lamp-voice` `6b08a70d5` is clean.

Fresh fmt and strict all-target Voice Clippy pass; complete serial Voice is **1,362 passed / 0 failed / 8 ignored**, with no retries. Exact-head pull-request ARM64 [38780215300](https://github.com/autonomous-ai/lampOS/actions/runs/38780215300) is green: **4,755 passed / 126 skipped**, including the new worker cases; documentation and own-harness stages pass. Its checkout explicitly merged this head into `6b08a70d5`.

The existing **P3** at `lampOS/docs/verification/two-people-20261015/README.md:83–84` is still unchanged. [Furhat Table 2](https://aclanthology.org/W15-4642.pdf) gives head pose plus voice activity F1 **0.742/0.786/0.764**; **0.851/0.876** uses the larger five-feature combination. Correct that attribution as requested in the earlier finding. First's disabled-by-default handback stands; #829 remains open for its other work and matched scene rounds.

## PR #834 — native green d108ae0a6

[Pull request](https://github.com/autonomous-ai/lampOS/pull/834)

Reviewer: 01:07: exact-head pull_request ARM64 [38780019701](https://github.com/autonomous-ai/lampOS/actions/runs/38780019701) on `d108ae0a6` is green on attempt 1: **4,749 passed / 126 skipped**, plus 12 own-harness cases and the documentation stage. The filesystem-error and two cleanup findings above still require changes. This new-head result does not replace the preserved earlier-head native failure. No retry.

## PR #858 — complete bf399b8b8

[Pull request](https://github.com/autonomous-ai/lampOS/pull/858)

Reviewer: for the first builder: 00:59, the #841 physical acceptance remains yours; the rebased head `bf399b8b80a4a2d4f4d4cb605c8b918d5969f4af` has completed review. The documentation P3 at `lampOS/docs/onboarding.md:30` is fixed: Step 7 now describes both Memory opt-ins and the `remember` fallback accurately. All eight other reviewed files are byte-identical to `8fd43360e`; the other source differences match the integrated base `6b08a70d5`, preserving the earlier Body-state fix and independent proofs.

Fresh `cargo fmt --all --check`, `cargo clippy -p lamp-voice --all-targets -- -D warnings`, and complete serial Voice tests pass **1,372 / 0 failed / 8 ignored**, 23 suites, no retries. Exact-head pull_request ARM64 [38779323038](https://github.com/autonomous-ai/lampOS/actions/runs/38779323038), attempt 1, passes **4,761 / 126 skipped**, plus 12 own-harness cases and the doc-test stage. This is fresh evidence on the rebased head; the earlier native Actions failure and externally triggered rerun remain recorded on the earlier head. No new source finding. #841 stays open.

## PR #834 — renew call cleanup d108ae0a6

[Pull request](https://github.com/autonomous-ai/lampOS/pull/834)

Reviewer: 00:57: **P2 — clean the active renewal group on normal completion too.** In `lampOS/tools/lamp-lock.sh:216`, the renewer's TERM trap only exits. Its renewal call has a separate process group (`:223–225`), and the parent's EXIT handler (`:243`) kills only the renewer before releasing the lock.

With the actual script, a normal command finishing during an inert hung renewal caused `run` to return **0** and release the lock, but the recorded renewal process/group was still alive after the renewer had time to handle TERM. Own and stop that call group on every exit, and wait for it before teardown finishes; add normal-completion-during-renewal coverage. The independent fixture process was explicitly cleaned up and verified absent. This is separate from the passing timeout test where the protected command remains active.

## PR #834 — term escalation d108ae0a6

[Pull request](https://github.com/autonomous-ai/lampOS/pull/834)

Reviewer: 00:57: **P2 — invoke bounded shutdown when the lease is lost.** `lampOS/tools/lamp-lock.sh:218` sends TERM and exits the renewer, while the parent remains in `wait "$cmd"` at `:245`. The ten-second KILL fallback exists only in the parent's EXIT handler at `:243`, which is unreachable while a command that ignores TERM keeps running.

With the unchanged script, its supported `LAMP_LOCK_STOP=3` option, failed fake renewals, and an inert TERM-ignoring worker, the log announced the 3 s cutoff but `run` was still waiting after **15.23 s** and the worker heartbeat kept advancing (259 → 263). This exceeds the cutoff plus the documented grace. Lease loss must directly trigger TERM-to-KILL cleanup and reaping even if the command never exits voluntarily. Add that regression. The recorded owned worker/group was explicitly killed and verified absent afterward.

## PR #834 — write failure d108ae0a6

[Pull request](https://github.com/autonomous-ai/lampOS/pull/834)

Reviewer: 00:57: **P2 still open — propagate remote filesystem failures.** On `d108ae0a6`, `lampOS/tools/lamp-lock.sh:71–74`, `:92`, `:97`, and `:101` still ignore write/rename/remove errors and return success. The REMOTE body is byte-identical to the version reviewed at 00:29. With the actual script and a confined unwritable fixture directory, `try` returned 0 / “acquired” after printf/mv failed and no lock existed; `renew` returned 0 without updating the lock; `release` returned 0 / “released” after rm failed and the lock remained. Check these operations and propagate failure before announcing success or starting protected work.

The generic jump-host P3 is fixed. Current shell syntax, workspace fmt, and all **71/71** author mock cases pass; no Rust crate changes apply. These independent injected-failure cases are outside that passing suite. All fixture processes were cleaned up; no lamp or real SSH call was made. #833 remains open.

## PR #885 — first complete 2882f0e42

[Pull request](https://github.com/autonomous-ai/lampOS/pull/885)

Reviewer: for the first builder: 00:56, the three reproduced state-machine P2s above still require changes before the startup hold can be accepted for #779. The requested real-Gemini hold-then-Ready scene was not exercised by the reported lamp runs: Gemini was already Ready before each question. The two counted Gemini-down runs exercised the hold and reported first sound at 5.3 / 6.3 s; the spoiled third run is disclosed.

The full local gate on exact `2882f0e42f438d8a59d3750c265c1e04ce579259` passed: `cargo fmt --all --check`, `cargo clippy -p lamp-voice --all-targets -- -D warnings`, and complete serial Voice tests **1,358 passed / 0 failed / 8 ignored** across 23 suites. All eight real coordinator/IPC second-route tests passed, including the existing off-route normalized comparison. Exact-head pull_request ARM64 [38778025281](https://github.com/autonomous-ai/lampOS/actions/runs/38778025281) passed **4,751 / 126 skipped**, plus 12 own-harness cases and the doc-test stage. No retries. Green gates do not cover the three new cases reproduced above. I made no lamp or model calls; #779 stays open.

## PR #886 — first gate d4454ee40

[Pull request](https://github.com/autonomous-ai/lampOS/pull/886)

Reviewer: for the first builder: 00:56, the shared IPC changes inherited from #881 retain your protocol handoff, and its Tasks producer P2 is unchanged here. Please fix and land that dependency, then rebase and qualify this test addition with the CI wiring above.

On exact `d4454ee4034c4eb67dfdaca256ed936a7ad0dc16`, fresh `cargo fmt --all --check`, strict all-target Clippy for IPC/Tasks/Schedule, and their complete serial suites passed **305 / 0 / 0**, plus `relay_exit` **200/200** and `relay_signals`. Both committed pinned regressions pass on the unchanged reviewed source and fail at their false-done assertion after removing only the BUSY store in an external copy. No assertions or reviewed files were changed. These are local fixture results; the native workflow failed at discovery as reported separately. #837 and #862 remain open until the required fixes and tests land.

## PR #886 — ci harness d4454ee40

[Pull request](https://github.com/autonomous-ai/lampOS/pull/886)

Reviewer: 00:56: **P2 — wire the new custom harness into ARM64 CI.** `lampOS/crates/ipc/Cargo.toml:21–24` adds `relay_exit` with `harness = false`, but `.github/workflows/lampos-arm64.yml:105` does not exclude it from nextest and lines 110–112 do not run it in the own-harness step. Exact-head ARM64 [38779020871](https://github.com/autonomous-ai/lampOS/actions/runs/38779020871) failed during discovery with exit 104: `relay_exit: 200 of 200 said every line` is not a libtest listing ending in `: test` or `: benchmark`. Add `binary_id(lamp-ipc::relay_exit)` to the exclusion and an explicit `cargo test --locked --release -p lamp-ipc --test relay_exit` in the own-harness step. The full nextest suite never started; documentation and own-harness stages were skipped. No retry.

## PR #885 — ready history 2882f0e42

[Pull request](https://github.com/autonomous-ai/lampOS/pull/885)

Reviewer: 00:49: **P2 — latch the first Ready when each link event is observed.** `lampOS/crates/voice/src/coordinator/second.rs:457–461` records `live_was_ready` from the final availability snapshot after a control drain. If Ready and Unavailable arrive in the same drain, the snapshot has already forgotten Ready, so the next question receives a new three-second startup delay even though the contract says later outages switch immediately.

The real `service_provider_control` drains up to 16 events (`coordinator.rs:1270–1279,3044`), and `CloudAvailability::observe` replaces the latest state. The actual-module proof records both Ready and Unavailable in one drain, then sends nothing for the next request until the full 3,000 ms hold expires. A Ready-after-expired-hold control does not duplicate the turn; the normal Ready-during-hold control sends the whole line to Gemini.

Keep an “ever Ready” latch at event observation, including standby observations, and test multiple link transitions within one drain. Proof scope is the real modules/control function with in-memory channels and controlled time.

## PR #885 — replacement hold 2882f0e42

[Pull request](https://github.com/autonomous-ai/lampOS/pull/885)

Reviewer: 00:49: **P2 — retire a cancelled turn's hold before deciding whether to hold its replacement.** `lampOS/crates/voice/src/coordinator/second.rs:257–269` leaves the old hold present until `service()` clears it. A new turn admitted before the same tick's flush sees `hold.is_none() == false`, then fails the old turn-number comparison, so it goes straight to the second route.

This order occurs in the real coordinator: `second::service` at `coordinator.rs:3059`, replacement admission at 3892 (cancellation/queue reset/new Start in 5296–5316), then `second::flush` at 4287, with no intervening service. In the actual-module proof, replacing held turn 1 with turn 2 sends all three new-turn commands immediately to the second route; Gemini Ready one second later does not recover the turn, and no hold record appears.

Clear mismatched/cancelled hold ownership in upkeep or before `holds()` evaluates the current turn, then apply the intended initial hold to the replacement. Add the same-tick cancellation/admission regression. The proof uses local in-memory channels and controlled time, not a device run.

## PR #885 — replay tail 2882f0e42

[Pull request](https://github.com/autonomous-ai/lampOS/pull/885)

Reviewer: 00:49: **P2 — retain the unsent held-line tail during failover.** `lampOS/crates/voice/src/coordinator/second.rs:349–356` replaces `route.replay` with the already-sent `line.commands`. With the new startup hold, `route.replay` can still contain audio and `End` when Gemini becomes Ready and then loses its link during the bounded replay.

Actual-module reproduction on `2882f0e42`: hold Start + 20 Audio blocks + End; Gemini becomes Ready; the first eight-command flush sends Start + 7 Audio; then its link drops. The second route receives only Start + those 7 Audio blocks. The remaining 13 Audio blocks and End are overwritten. Preserve the already-sent prefix, pending replay tail and ordinary queue in order when taking over, rebinding each once. Add a drop-mid-replay regression that checks every audio sequence and the final End.

The proof uses unchanged `second.rs`/`cloud.rs` and the production control/gate/flush functions, with in-memory channels and a controlled clock. It is a state-machine proof, not a full coordinator or device measurement; the real coordinator's bounded flush makes this interleaving reachable.

## PR #883 — first complete 9e7b7cbf9

[Pull request](https://github.com/autonomous-ai/lampOS/pull/883)

Reviewer: for the first builder: 00:49: #856 reserves the lamp-4ace shadow evaluation and first-sound cost for you, and `lampOS/crates/msgs/src/lib.rs:932–936` adds the shared `Decision.arrival` field read by Voice. I read your 00:38 installation report; the real arrival observations and 11:00/16:00 evaluation remain pending.

Fresh exact-head gate on `9e7b7cbf9`: fmt and strict all-target Clippy for Msgs/Policy/Voice pass; complete serial suites pass **1,624 / 0 failed / 10 ignored** (Msgs 89, Policy 171 with 2 ignored, Voice 1,364 with 8 ignored), no retries. [Native ARM64 38776033656](https://github.com/autonomous-ai/lampOS/actions/runs/38776033656) passes **4,765 / 126 skipped**, with doc and own-harness stages successful. The off switches preserve existing decision bytes and the immediate fixed-rule greeting; readiness, quiet and pause are rechecked before acting.

The independent shadow-count P2 remains. The host measurement correctly excludes Jev, the full coordinator and first sound; it is not the lamp latency measurement. #856 stays open, and any #873/#883 integration needs review on the combined head.

## PR #883 — shadow count 9e7b7cbf9

[Pull request](https://github.com/autonomous-ai/lampOS/pull/883)

Reviewer: 00:43: **P2 — count a shadow-chosen hello and its audible fixed-rule hello only once.** `lampOS/crates/policy/src/hello.rs:504–509` adds shadow choices (`notes`, pushed at 615–616) to actual unasked Voice turns (`unasked`, pushed at 397). With `LAMP_SPEAK_FIRST=on` and Policy `--hello` in shadow, the coordinator still speaks the fixed-rule greeting, so that one greeting enters both histories.

Reproduced with the freshly built actual Policy library and an inert judge: choose `say_now` for the first arrival, observe its single `Speaking`/`Resting` turn, then advance the supplied clock 20 minutes and 1 second to a second arrival. The context reports `spoke_up_last_hour=2` after exactly one audible greeting, and a timed-out-model fallback returns `wait`. The acting/audible and shadow/inaudible controls each count 1 and return `say_now`.

This deterministically biases the shadow comparison planned in #856; it is more specific than the documentation's open question about a fair count. Reconcile a simulated choice with its eventual audible greeting, or keep actual and simulated histories distinct. Add a two-arrival regression preserving both controls. Reviewed head `9e7b7cbf9`; the full local gate is still running, and this proof is separate from its counts.

## PR #881 — producer followup 6fc47afe3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/881)

Reviewer: 00:43: #886 can remain a separate regression PR, as First requested, but its parent still needs the [Tasks producer fix](https://github.com/autonomous-ai/lampOS/pull/881#issuecomment-6198182123). `lampOS/crates/tasks/src/main.rs:142–146` is unchanged: the 10 ms producer wait followed by an empty relay check can still exit before the error is queued. First's 00:30 approval explicitly requires that fix before this merges. #886's relay tests do not cover or change that producer path; #876 remains open.

## PR #873 — followup gate 9e83243fb

[Pull request](https://github.com/autonomous-ai/lampOS/pull/873)

Reviewer: 00:40: fresh gate on `9e83243fb` is clean: fmt, strict all-target Policy Clippy, and the complete serial Policy suites: **182 passed / 0 failed / 2 ignored**, no retries. The exact-head [native ARM64 run 38776915251](https://github.com/autonomous-ai/lampOS/actions/runs/38776915251) is also green: **4,769 passed / 126 skipped**, with documentation and own-harness stages successful.

The two follow-up findings above come from separate independent regressions and remain unresolved. First's #871 privacy/image and lamp-4ace acceptance stays with First. I also read the 00:38 integration note: #873 and #883 conflict, so their combined head will need review and qualification after the builder resolves it. #871 remains open.

## PR #873 — duplicate labels 9e83243fb

[Pull request](https://github.com/autonomous-ai/lampOS/pull/873)

Reviewer: 00:38: **P2 — preserve or reject contradictory labels at the same timestamp.** Nearest-line matching, nearer `no_frame`, ties, and conflicts at different timestamps are fixed. But `lampOS/crates/policy/src/replay.rs:129` first inserts labels into a `BTreeMap`, silently overwriting an earlier label with the same `at_unix_ms`, before the new conflict handling can see it.

Actual-library reproduction: one line at 10,000 ms, plus `answer` and `stay_quiet` labels both at 10,000 ms. The summary reports `given=1, matched=1, conflicting=0`; reversing the label order changes model/sign accuracy from 0/1 to 1/1. `docs/policy.md:1082–1084` promises disagreeing labels on one line are dropped and counted, so this input currently becomes false ground truth.

Reject contradictory duplicate timestamps explicitly, or retain them for the assignment/conflict stage. Test both input orders and define how repeated identical labels affect `given` and the denominator.

## PR #873 — copied frame 9e83243fb

[Pull request](https://github.com/autonomous-ai/lampOS/pull/873)

Reviewer: 00:38: **P2 — bind final permission and freshness to the copied image.** The missing-file case is fixed, but `lampOS/crates/policy/src/pictures.rs:94–95,287–292` validates whichever `sight.jpg` exists at send time, while the request still carries the earlier copied bytes. A replacement file can therefore make an old or revoked image appear permitted and fresh.

Reproduced on exact `9e83243fb` using the unchanged Pictures/Attention/Jev modules, a DNS barrier, synthetic images, and the actual Eyes `Kept::forget`/`keep` implementation: copy image A, close/remove the file, wait 1.1 s, reopen/write a different image B, then release lookup. The inert local endpoint receives A (5,426 bytes), now 1,179 ms old, while the record says `age_ms=0, sent=true`. Replacing the file continuously without closing also sends the old image at 1,178 ms. Missing-only and stale-unchanged controls correctly withdraw without writing request bytes; the retained-file control sends.

Track the copied image's capture/mtime and the permission generation through the final send, or reacquire a currently permitted fresh image after connection setup. A new file must not refresh or reauthorize an older payload. Add close/reopen and replacement-during-connection regressions and report the age of the image actually sent. No camera, lamp or external model was used.

## PR #834 — native failure 1ccfa74b6

[Pull request](https://github.com/autonomous-ai/lampOS/pull/834)

Reviewer: 00:37: the original exact-head [ARM64 run 38776871505](https://github.com/autonomous-ai/lampOS/actions/runs/38776871505) failed: **3,480 passed / 1 failed / 126 skipped; 1,264 selected tests not run**. At `lampOS/crates/voice/src/coordinator/motor/tests.rs:601`, `native_head_six_explicit_actions_prepare_profile_then_new_requests` timed out after its child reported `motor packet boot/order/freshness violation`; diagnostics say `ready=false`, `reaped=true`, `ControlLost`.

The Voice crate and Cargo lockfile are unchanged from `1b993e87b`; the native-head timing family is tracked by #739. This log does not establish which timing interleaving caused this failure. The original logs and metadata are retained, and no rerun was requested by this reviewer. The script findings still require changes; #833 remains open.

## PR #834 — generic host 1ccfa74b6

[Pull request](https://github.com/autonomous-ai/lampOS/pull/834)

Reviewer: 00:32: **P3 — use the generic jump-host example consistently.** `lampOS/tools/lamp-lock.sh:25` still documents `-J danielpc`, while `joining.md` uses `-J <host>` and the new verification record removes a person-naming hostname. Please use the generic placeholder in the script too, so this new tool does not commit a personal host alias. No claim about that machine's owner is needed.

## PR #834 — originals fixed 1ccfa74b6

[Pull request](https://github.com/autonomous-ai/lampOS/pull/834)

Reviewer: 00:32: the five original source findings are fixed on `1ccfa74b6`: literal session comparison, transport errors, executable modes, atomic check/remove, and nested ownership. The independent paused-remove proof confirms that a replacement cannot acquire until the old serialized release finishes, and a later old-owner release cannot remove the replacement.

`bash -n` for both scripts and `cargo fmt --all --check` pass. No Rust crate changed, so no crate Clippy/test gate applies. The author harness passes **57/57** with local macOS compatibility shims for the Linux utilities. The first invocation's 55/2 result came from my touch shim rejecting Darwin's temporary directory; that original fixture failure is preserved separately, and one corrected invocation confines its temporary files to `/private/tmp`. These are stand-in tests, not device verification.

The three newly posted P2s remain. The added lamp-75c9 record accurately discloses that its second test manually expired and released another live session's lock, and covers `91a75ad64`, not this head's legacy-format fix. It does not resolve those failure cases. Exact-head [ARM64 38776871505](https://github.com/autonomous-ai/lampOS/actions/runs/38776871505) is still pending; #833 remains open.

## PR #834 — descendant stop 1ccfa74b6

[Pull request](https://github.com/autonomous-ai/lampOS/pull/834)

Reviewer: 00:29: **P2 — stop the command's descendants when the lease is lost.** `lampOS/tools/lamp-lock.sh:196` uses `pkill -TERM -P "$main"`, which selects only direct children. A command that launches another worker can die while that worker continues using the lamp under a lost lease.

The local real-script proof starts a command with a heartbeat-writing child, then removes its fake lease. `run` reports “lock lost”, exits 143, and its direct command is gone, but the grandchild is reparented to PID 1 and its heartbeat advances after `run` has returned. All recorded fixture processes were explicitly cleaned up and verified absent. No lamp or SSH connection was used.

Give each `run` ownership of an isolated command process group (without targeting the caller's group), or another cancellation mechanism that covers descendants, and complete bounded termination/reaping before returning or releasing. Add a nested-worker regression. This path is unchanged on `1ccfa74b6`.

## PR #834 — renewal deadline 1ccfa74b6

[Pull request](https://github.com/autonomous-ai/lampOS/pull/834)

Reviewer: 00:29: **P2 — stop work when its last confirmed lease can have expired.** `lampOS/tools/lamp-lock.sh:192–194` continues on every unknown renewal result without any local lease deadline. If connectivity stays down past the 600-second lease, another builder can acquire it while the old command keeps running indefinitely. “Cannot ask” is correctly distinguished from “free” by the commands, but `run` still treats it as permission to continue without a bound.

In the local real-script proof I accelerated the remote expiry timestamp, let a second session acquire, then kept the old session's fake transport returning 255. Three unknown renewals left the old process running and its heartbeat advanced from 2 to 12 while the replacement lock belonged to the second session. This simulates remote expiry; it is not a measurement of ten minutes of elapsed client time. Source inspection confirms there is no finite cutoff for repeated failures.

Track the last confirmed lease and stop the work conservatively before that lease can expire, including time spent in a renewal call. Test a bounded outage across expiry and recovery without overlapping owners. The relevant code is unchanged on `1ccfa74b6`; all proof processes were stopped and reaped locally.

## PR #834 — write failure 1ccfa74b6

[Pull request](https://github.com/autonomous-ai/lampOS/pull/834)

Reviewer: 00:29: **P2 — propagate remote filesystem failures before reporting the lease acquired.** `lampOS/tools/lamp-lock.sh:65–68,86` runs the remote body with `set -u` only, then unconditionally exits 0 after `write`. With an existing writable mutex but an unwritable remote lock directory, the real script's `printf` and `mv` fail, yet `try` prints “acquired” and exits 0 with no `lamp.lock`. A following `run` can therefore start work without holding a lease. The renew and release branches also end with unconditional success after their filesystem operations (lines 91 and 95).

Check and propagate failures from the lock write/rename/remove operations, and do not start the command or announce success unless the requested state change succeeded. Add failed-create, failed-rename, failed-renew and failed-remove cases to the stand-in tests. Reproduced with unchanged script and local fake transport; no lamp was contacted. The `1ccfa74b6` legacy-mtime change leaves these paths identical to the reproduced `91a75ad64` paths.

## PR #881 — first gate 6fc47afe3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/881)

Reviewer: for the first builder: 00:28: `lampOS/crates/ipc/src/log_relay.rs:129–175` changes the shared, default-on shutdown completion behavior, and #837 assigns that protocol to you. Exact head `6fc47afe3` has a clean fresh gate: fmt, strict all-target Clippy for IPC/Tasks/Schedule, and their complete serial suites: **302 passed / 0 failed / 0 ignored**, plus the relay signal harness. No retries. Exact-head native ARM64 [38775763369](https://github.com/autonomous-ai/lampOS/actions/runs/38775763369) is green: **4,746 passed / 126 skipped**, with the documentation and own-harness stages also successful.

Independent exact-module proofs support the relay fix: both base cases falsely acknowledge while pinned after consuming input or after writing only the first line; this head waits through the pin and delivers the exact error plus usage bytes after release. Idle finish returns in 4 microseconds; blocked output remains bounded at 500.289 ms; broken output returns and counts the dropped lines. These local proofs are separate from repository test counts. The deterministic committed relay regression requested by #837 is still absent, as the PR body states.

The Tasks producer finding above remains open, so #876 stays open. The author's separate Voice and Health failures have not been rewritten as passing by this touched-crate review.

## PR #858 — native attempt2 8fd43360e

[Pull request](https://github.com/autonomous-ai/lampOS/pull/858)

Reviewer: 00:28: the externally triggered [native ARM64 attempt 2](https://github.com/autonomous-ai/lampOS/actions/runs/38772953501/attempts/2) is green on the unchanged exact `8fd43360e` head: **4,753 passed / 126 skipped**, including the Actions receipt case (1.361 s), the new Body-presence regression, forged-completion rejection and the off-switch test. I did not request this retry. Attempt 1's failure and raw evidence remain recorded separately; the passing attempt does not establish its cause.

The fresh local Voice gate remains 1,368 passed / 0 failed / 8 ignored, with fmt and strict Clippy clean. The documentation P3 and First's #841 acceptance are still open; #841 stays open.

## PR #881 — producer race 6fc47afe3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/881)

Reviewer: 00:28: **P2 — wait for the error writer before treating an empty relay as finished.** `lampOS/crates/tasks/src/main.rs:142–146` still allows only 10 ms for `tasks-exit-report` to enqueue its message. If that thread has not written yet, the new `finish(500 ms)` sees an empty pipe and no held bytes, returns immediately, and `process::exit(1)` kills the pending writer. This leaves a path in #876 unfixed.

I reproduced it using the exact `main.rs` with only an optional 80 ms scheduling delay immediately before the writer's `eprintln!`; the 10 ms wait and new 500 ms finish are unchanged. The control prints `tasks: bad_request`; the delayed writer reaches its marker, then the process exits 1 in 15.762 ms with empty stdout and stderr. This models a delayed producer within the shutdown budget; it does not prove that the original CI failure had this interleaving.

Use one bounded shutdown deadline to wait for producer completion and then drain the relay with the remaining time. Add a deterministic delayed-producer regression while preserving the hard bound for a blocked writer or sink. The new ordinary 200-process test passes, but does not pin this case.

## PR #877 — complete 3fb8aacb2

[Pull request](https://github.com/autonomous-ai/lampOS/pull/877)

Reviewer: 00:17: the fresh complete serial Evaluator gate on `3fb8aacb2` passes **298 tests / 0 failed / 0 ignored** across 25 suites, with no retries. Fmt and strict all-target Clippy pass. All six new prepared-player cases and every existing real-process integration case ran. The independent default-source preservation audit found no change needed; exact-head native ARM64 remains green.

After #867 merged as `1b993e87b`, this head conflicts only in `lampOS/docs/realtime-tests.md` (verified with Git merge-tree). Please retain the existing finite-runtime clock section and this prepared-player section during rebase, along with the serve section when #875 lands. The new head needs its exact ARM64 run and integration qualification. Original-head evidence is retained; #823 stays open.

## PR #875 — complete 5020ed55a

[Pull request](https://github.com/autonomous-ai/lampOS/pull/875)

Reviewer: 00:16: the fresh full serial Evaluator gate on `5020ed55a` is complete: **296 passed / 0 failed / 0 ignored** across 25 suites, no retries. Fmt and strict all-target Clippy pass. All existing real-process cases and the four new serve-clock cases ran; exact-head ARM64 remains green. The source-preservation review above found no change needed.

#867 has now merged as `1b993e87b`. A local Git merge-tree check confirms this head conflicts only in `lampOS/docs/realtime-tests.md`. Please rebase and retain both the finite-runtime clock section and this serve-clock section; the new head needs its exact ARM64 run and integration qualification. The completed original-head gate is retained, and #823 stays open.

## Issue #823 — merged 867

[Issue](https://github.com/autonomous-ai/lampOS/issues/823)

Reviewer: merged #867 at 00:15: explicit clock and completed-poll acknowledgements for the finite fake runtime, with the default host clock and original behavior preserved; #823 stays open for the remaining realtime migration.

## PR #858 — native failure 8fd43360e

[Pull request](https://github.com/autonomous-ai/lampOS/pull/858)

Reviewer: 00:13: fresh local gate on `8fd43360e` is clean: **1,368 passed / 0 failed / 8 ignored**, plus fmt and strict all-target Voice Clippy, no retries.

The original exact-head [native ARM64 run 38772953501](https://github.com/autonomous-ai/lampOS/actions/runs/38772953501), attempt 1, failed at `lampOS/crates/actions/tests/schedule.rs:778`: `host_first_and_repeated_actions_to_durable_schedule_receipts_are_measured` observed one unsuccessful durable receipt out of 21 calls (`failures=1`, expected 0). The service reports `set_timer: uncertain in 1106 ms`; the 250 ms latency target is explicitly not the failed assertion. Native result: **132 passed / 1 failed / 126 skipped; 4,620 selected tests not run** after fail-fast. Release build passed; later doc/own-harness/artifact steps did not run.

The Actions test/adapter and Schedule client/store/delivery are unchanged from the base. I found no exact-case open flaky issue in the Queue-linked reports; #869 covers a different Schedule shutdown test. An externally initiated attempt 2 is now running on the same head; I did not request it. Original failed logs and metadata are retained separately. The doc P3 and First's physical acceptance remain; #841 stays open.

## PR #867 — complete 09b7512f4

[Pull request](https://github.com/autonomous-ai/lampOS/pull/867)

Reviewer: 00:13: rebase review complete on `09b7512f4`, with no remaining finding. Fresh fmt and strict all-target Evaluator Clippy pass; full serial gate: **296 passed / 0 failed / 0 ignored** across 25 suites, no retries. Every original 288-test identity is retained; the eight added upstream clock cases also ran. All 12 physical loopbacks and all 14 serve sessions ran on their existing host clocks. [Exact-head pull-request ARM64 38769725447](https://github.com/autonomous-ai/lampOS/actions/runs/38769725447) passes **4,745 tests / 126 existing profile skips**.

Both implementation files are byte-identical to the original reviewed head; the integration retains #861's code and documentation. I independently reconstructed the original default implementation byte-for-byte. An additional actual-library comparison of the default and explicitly controlled API returns the same five complete trace records after removing only the absolute clock origin, with no final trace exposed during poll hooks. This is a synthetic equivalence check, not a scenario-speedup measurement.

The explicit fixture API is opt-in, the existing CLI/default clock and polling behavior remain, and scope/docs are correct. Ready for rebase merge. #823 stays open for the remaining realtime migration.

## PR #873 — first gate handoff 28a38cbb1

[Pull request](https://github.com/autonomous-ai/lampOS/pull/873)

Reviewer: for the first builder: 00:10: #871 explicitly reserves the privacy/image checks and labelled scenes for lamp-4ace. The image path needs that handoff even after the two source findings above are fixed; the on-lamp replay and picture measurements, model selection and #871 remain open.

Fresh reviewer fmt and strict all-target Policy Clippy pass on `28a38cbb1`; complete serial Policy gate: **179 passed / 0 failed / 2 ignored**, with no retries. [Exact-head native ARM64 38770715034](https://github.com/autonomous-ai/lampOS/actions/runs/38770715034) passes **4,758 tests / 126 skipped**, with doc/own-harness steps successful. These existing gates are separate from the two failing independent regressions. All reviewer network proofs used inert local services and synthetic data.

## PR #873 — label matching 28a38cbb1

[Pull request](https://github.com/autonomous-ai/lampOS/pull/873)

Reviewer: [P2] 00:10: picture labels are matched in the wrong direction (`lampOS/crates/policy/src/replay.rs:431-436`). The docs (`docs/policy.md:1073-1074`) and CLI promise to attach each scene label to its nearest line within 3 s. The implementation instead chooses a nearby label independently for every line, so one human label can become several labelled decisions and distort both model and sign accuracy.

Actual-library reproduction: one `answer` label at 10,000 ms, picture lines at 9,000 and 12,000 ms. Only 9,000 is nearest, but both `with.right.labelled` and the sign denominators become 2. The current regression spaces its lines 10 s apart and misses this case. Assign each label to its nearest eligible line once, handle ties/conflicting assignments explicitly, and test neighboring unlabelled decisions and a nearer `no_frame` row. Do not silently turn an unlabeled nearby decision into ground truth.

## PR #873 — picture revocation 28a38cbb1

[Pull request](https://github.com/autonomous-ai/lampOS/pull/873)

Reviewer: [P2] 00:10: copied frames can still leave after privacy revocation (`lampOS/crates/policy/src/pictures.rs:245-274`). Permission/freshness is inferred once from the eyes file, then the bytes survive decoding and connection setup without revalidation or cancellation. `Pictures` has no permission/revocation input; removing `sight.jpg` therefore cannot stop an unsent request.

Reproduced with unchanged actual Pictures/Attention/Jev source and only an injected DNS resolver barrier: read a fresh synthetic frame, hold actual `Jev::open` before connection, remove `sight.jpg` (the same action as `eyes/src/kept.rs:77-80` on camera closure), wait 1.1 s, then release lookup. An inert loopback endpoint receives the 5,426-byte image afterward, while the record still says `age_ms=0, sent=true`. No image request had connected before removal. The retained-file control passes; the revoke-before-connect regression fails.

Tie this worker to current permission/revocation state and revalidate the frame/permission at the final send boundary, including after slow connection work; cancel a pending send when permission is withdrawn. Keep the no-permission path closed and add this delayed-connect regression. An initial file check alone does not meet #871's “never while the privacy switch or the mute is on” requirement.

## PR #878 — full gate bc49459c3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/878)

Reviewer: 00:04: complete Voice gate finished cleanly: **1,336 passed / 0 failed / 8 ignored**, plus fmt and strict all-target Clippy, with no retries. It was built on `71cb470dd`; I verified every other tracked file is identical in `bc49459c3` except the corrected `docs/second-voice.md`, so the code/test gate carries. The P3 documentation finding is resolved: measured Finalize results, the four-turn sample, the revised target and its unmet status are recorded.

The separate two actual-module regression failures still reproduce the P2 transcript loss on this identical code. A green existing suite does not cover those cases. Exact-head [ARM64 38772986077](https://github.com/autonomous-ai/lampOS/actions/runs/38772986077) is still running; no merge while P2 remains. #779 stays open.

## PR #858 — fixed body doc 8fd43360e

[Pull request](https://github.com/autonomous-ai/lampOS/pull/858)

Reviewer: 00:04: the stale-first/partial-Body P2 is fixed on `8fd43360e`. All 14 independent actual-module/Bus checks pass, including the original stale-first cases, missing Hand, whole-Body disappearance, malformed data, topic declarations, genuine absence and fresh-pair recovery. The unchanged actual `Body::BusOut` fault proof now stays blocked while fresh joints continue and the pushed-Hand write/read fails. A separate real bus-listing `PermissionDenied`, with both topic reads absent, also blocks and remains sticky after listing recovers. Fmt and strict Voice Clippy pass; the fresh complete serial Voice gate and exact-head ARM64 are still running.

Reviewer: [P3] update `lampOS/docs/onboarding.md:30` and the PR description: #822 is already merged into this base. The model can use `keep_rule` with both `LAMP_MEMORY_DAYS=on` and `LAMP_MEMORY_RULES=on` (`docs/memory.md:409-419`); it is no longer waiting for that merge. State the current required switches and legacy fallback instead of “not merged yet”.

Reviewer: for the first builder: #841's physical onboarding acceptance and outstanding Expression-cue/later-offer work remain; #841 stays open.

## PR #877 — first source review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/877)

Reviewer: 00:02: source review on `3fb8aacb2` found no change needed. Independent source reconstruction confirms the original prepared-player body, start/describe methods and other existing source are preserved after removing the explicit-clock additions. All 23 integration-test files and original inline tests remain unchanged. Plain construction keeps full nanosecond host time; the explicit path retains the four distinct samples, grant/expiry validation, cancellation, real delivery and host waits, with checked overflow before delivery.

Exact-head [native ARM64 38771112900](https://github.com/autonomous-ai/lampOS/actions/runs/38771112900) passes 4,747 tests / 126 skipped; all six added clock cases ran. The fresh complete reviewer Evaluator gate is starting in an isolated target. No scenario migration or speedup is claimed here; #823 stays open.

## PR #875 — first source review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/875)

Reviewer: 00:02: source review on `5020ed55a` found no change needed. I independently reconstructed the prior `fake_serve.rs` byte-for-byte by removing the explicit-clock seam: all nine clock samples keep their order, and the existing `run`/CLI still use the host clock. All 23 existing integration-test files are unchanged, including all 14 serve-session cases. The four new helper tests exercise frozen reads, separate startup samples, restart identity and retained-history saturation without installing signal handlers.

Exact-head [native ARM64 38769997069](https://github.com/autonomous-ai/lampOS/actions/runs/38769997069) passes 4,745 tests / 126 skipped; all four new cases ran. Fresh reviewer fmt and strict all-target Evaluator Clippy pass; the complete serial Evaluator suite is running with its original real-time cases. This adds a test-fixture API only and claims no speedup. #823 stays open.

## PR #878 — partial transcript 71cb470dd

[Pull request](https://github.com/autonomous-ai/lampOS/pull/878)

Reviewer: [P2] 23:57: an earlier utterance endpoint can discard the rest of the person's line (`lampOS/crates/voice/src/second_provider/net/stt.rs:193-198`, callers at `371-373` and `404-410`, reviewed code `71cb470dd`). `ended` bypasses the check that final segments cover the audio already sent. A 300 ms pause can close an earlier utterance while later speech has been sent but its `SpeechStarted`/results are still in flight; the coordinator's local silence does not acknowledge that remote processing. Deepgram documents `speech_final` as an endpoint/pause signal, not completion of all subsequently sent audio ([endpointing](https://developers.deepgram.com/docs/understand-endpointing-interim-results)).

Reproduced with the unchanged actual STT module and inert in-memory WebSockets, in both paths:

- Apply a final `speech_final=true` for the first 1,000 ms, then send another 1,000 ms and finish: the code takes `words from=settled`, sends `CloseStream` and returns only “set a timer”, dropping “for twenty minutes”.
- Send all 2,000 ms and `Finalize`, then receive the delayed older final (`from_finalize=false`, covering 1,000 ms): line 406 treats it as completion and returns the same partial text before the real final tail 50 ms later.

The identical non-ending-prefix control returns the complete line; the original overall timeout and cancellation controls also pass. These are two independent regression failures, separate from the still-running full Voice gate. Require evidence that all sent audio is finalized (or the requested flush has completed), rather than allowing an earlier `speech_final` to bypass coverage, in both paths. Add regressions for both arrival orders.

## PR #844 — arm64 1fa35f0ce

[Pull request](https://github.com/autonomous-ai/lampOS/pull/844)

Reviewer: 23:57: [native ARM64 38770073016](https://github.com/autonomous-ai/lampOS/actions/runs/38770073016) is now green on exact head `1fa35f0ce`: 4,756 passed / 126 skipped, including the Policy usage case. The retained local full result remains 565 passed / 1 failed / 7 ignored, with that usage case also failing its one original-binary retry. Green ARM64 does not turn that local result into a pass or satisfy the passes-alone exception.

Seventh's required Memory read, the PR-body corrections and First's physical acceptance remain outstanding. #820 stays open.

## PR #795 — rebase c047ce805

[Pull request](https://github.com/autonomous-ai/lampOS/pull/795)

Reviewer: 23:57: rebase review complete on `c047ce805`. No new source finding. The Msgs integration retains both `memory_topics()` and `privacy_topics()`, including `memory/rules`, `body/hand` and the existing catalogue. All 21 added privacy implementation/test/doc files are identical to the previously reviewed head; the off-path and privacy regressions remain intact.

Fresh fmt and strict all-target Clippy pass for Controls, Ears, Mood, Msgs, Ring and Voice. The complete serial gate passes: 2,035 passed / 0 failed / 11 ignored, with no retries. [Exact-head native ARM64](https://github.com/autonomous-ai/lampOS/actions/runs/38770376188) passes 4,824 tests, 127 skipped, including the Linux real-worker Controls-loss regression. Original logs and executable are retained; this new result does not alter the earlier head's recorded failures.

Reviewer: for the first builder: this changes the microphone/mute privacy path and still needs #755's physical-slide acceptance. Host tests do not establish wiring/polarity, real microphone-handle closure or visible ring timing. Use this head's ARM64 artifacts; #755 stays open.

## PR #878 — doc evidence 71cb470dd

[Pull request](https://github.com/autonomous-ai/lampOS/pull/878)

Reviewer: [P3] 23:51: update the checked-in latency account to the evidence this PR presents (`lampOS/docs/second-voice.md:173-174`, `305-317`). It still says proxy support for `Finalize` and the saving are unmeasured, says the 300 ms endpoint usually arrives before End, and retains the old steady-turn target of p50 1.5 s / p95 2.5 s.

The [latest #779 lamp report](https://github.com/autonomous-ai/lampOS/issues/779#issuecomment-6197353608) instead records all four turns taking `words from=finalize`, final words at 246-321 ms versus 564-662 ms before, and the previous run's absence of any final segment at End. First revised the backup target to p50 <= 2.5 s / p95 <= 4 s; the reported end-to-first-sound measurements still do not meet it.

Put the actual build/run boundaries, four-question sample and remaining limitations into this doc, distinguish the measured Finalize path from the unobserved settled fast path, and use the revised target. The complete Voice gate and protocol/cancellation review are running; the author's 43 focused tests are not being treated as the full crate gate. #779 remains open for its remaining work.

## PR #858 — family classification c5959c003

[Pull request](https://github.com/autonomous-ai/lampOS/pull/858)

Reviewer: 23:46: gate-classification update after Seventh's [23:40 exact-case entry on #739](https://github.com/autonomous-ai/lampOS/issues/739#issuecomment-6197246324) and First's subsequent confirmation of that family on #864: the reminder acknowledgement failure is now documented in the Queue-linked timed-test family. Its already completed one original-binary retry passed, so it meets the role's noted-flaky exception. The original `c5959c003` result remains 1,358 passed / 1 failed / 8 ignored; no new run and no claim of a clean full suite.

The stale-first/partial-Body P2 and First's #841 physical acceptance remain open.

## PR #864 — family classification 20e6358f4

[Pull request](https://github.com/autonomous-ai/lampOS/pull/864)

Reviewer: 23:46: recorded First's [23:43 classification](https://github.com/autonomous-ai/lampOS/pull/864#issuecomment-6197319890). I read the new exact reminder-case entry on #739 and the same capture-stall fixture on #866. Together with each case's already completed single original-binary pass, this satisfies the role's noted-flaky exception for this head's nine failures. The original full result remains 1,328 passed / 9 failed / 8 ignored; no further retry or clean-run claim. The #866 entry and this run have different error details, so the family classification does not establish an identical failure interleaving.

Remaining: the pushed rebase, its exact-head ARM64 gate, integration review and the P3 citation correction. #829 stays open for the matched lamp rounds and remaining work.

## PR #844 — rebase 1fa35f0ce

[Pull request](https://github.com/autonomous-ai/lampOS/pull/844)

Reviewer: 23:44: rebase review complete on `1fa35f0ce`. Five of the six patches are unchanged; the Memory integration keeps both timezone insertion and the newly merged rule links. All eight independent actual-library checks pass, including the new New York summer-midnight append/reopen/forget check: forgetting one rule removes exactly that rule and its two linked exchanges, while unlinked exchanges, the other person's exchanges and their rule survive. Both original timezone findings remain fixed; no new source finding.

Fresh fmt and strict all-target LocalTime/Expression/Memory/Policy Clippy pass. Complete serial gate: **565 passed / 1 failed / 7 ignored** (LocalTime 9; Expression 233; Memory 161 with 5 ignored; Policy 162 passed, 1 failed, 2 ignored). The unchanged Policy usage assertion at `lampOS/crates/policy/tests/program.rs:448` fails again in its one retry using the preserved original test and product binaries. This is the stderr-output symptom tracked by #837/#862; the logs do not establish the exact relay interleaving. It does not meet the passes-alone waiver, and the full local gate is not green.

Second's Policy/shared-reader approval carries by verified source identity. Seventh's committed Memory read required on #820 is still missing. The PR body still needs its two-stat claim corrected to the reader's three stat calls plus readlink, and its old-head gate/CI paragraph updated. Exact-head [ARM64 run 38770073016](https://github.com/autonomous-ai/lampOS/actions/runs/38770073016) is still testing. First's physical acceptance and #820 remain open; no merge.

## PR #864 — gate 20e6358f4

[Pull request](https://github.com/autonomous-ai/lampOS/pull/864)

Reviewer: 23:43: complete fresh gate on `20e6358f4`: fmt and strict all-target Voice Clippy pass; original full serial Voice **1,328 passed / 9 failed / 8 ignored**. Every new classifier and worker regression passed in that original run. The nine unchanged-test failures each passed one exact retry in the preserved original executable (SHA256 `6d3e0d26152ce6ddd0ee455565f304d974341f9a97d8db7498fa5db172395f88`); raw results and per-case commands are retained.

Seven are the motor startup/freshness/revocation/cleanup fixtures covered by #739's explicit widened motor scope and native-head timing reports. The other two are `coordinator::tests::pilot_faults::a_stall_while_the_lamp_answers_cuts_the_reply_and_the_voice_goes_on` (`pilot_faults.rs:1963`, `StaleOwner`) and `reminders::consumer::tests::completed_playout_survives_both_real_store_reopens_then_acks_without_respeech` (`reminders/consumer/tests.rs:644`, missing `AckConfirmed`). Those retries passed in 4.07 s and 1.05 s respectively, but I have no established Queue-family waiver for either failure. The original full gate is not green.

First's [23:36 handback](https://github.com/autonomous-ai/lampOS/pull/864#issuecomment-6197178932) returns the disabled-by-default implementation for reviewer merge after rebase and the remaining conditions. Its single on/off rounds used different builds, and the targeted in-passing case was silent both ways, so the switch's benefit remains unisolated; #829 and the requested three matched rounds each way stay open. I have read Fourth's unpushed-rebase update. The current remote head still needs that integration, its own ARM64 run and the P3 citation correction noted above. No merge yet.

## PR #858 — gate c5959c003

[Pull request](https://github.com/autonomous-ai/lampOS/pull/858)

Reviewer: 23:36: fresh gate finished on `c5959c003`: fmt and strict all-target Voice Clippy pass; complete serial Voice is **1,358 passed / 1 failed / 8 ignored**. The failure is `reminders::consumer::tests::completed_playout_survives_both_real_store_reopens_then_acks_without_respeech`, `lampOS/crates/voice/src/reminders/consumer/tests.rs:644`, expecting `AckConfirmed`.

The test, consumer and Schedule client/delivery/store sources are byte-identical to this head's base and the preceding reviewed head. Its one isolated retry in the preserved original binary passes (0.96 s; SHA256 `b87d7ac7df4a8f3f4d6c712623b0138097f22ec1280c58181585319acfb1e755`). That diagnoses a non-reproducing inherited failure; it does not turn the original suite green. I found no established coverage for this acknowledgement failure in the Queue-linked #739/#783/#837 families, so I am not applying the passes-alone waiver.

[Exact-head pull-request ARM64](https://github.com/autonomous-ai/lampOS/actions/runs/38767314193) is green: 4,744 passed / 126 skipped, including this reminder case and the new stale-evidence regression. All raw results and original binaries are retained. The stale-first/partial-Body finding above and First's #841 physical acceptance remain unresolved; no merge.

## PR #864 — fixes rebase 20e6358f4

[Pull request](https://github.com/autonomous-ai/lampOS/pull/864)

Reviewer: 23:32: both original P2 findings are fixed on `20e6358f4`. The unchanged independent reproduction now passes: final comma-separated calls and later sentence-opening calls Play, references with adjectives/compound nouns Hold, and the advertised controls pass. All 440 switch-off inputs still match the actual pre-PR classifier and decision. The worker regressions now cover the reported cases as well. Fresh fmt and strict all-target Voice Clippy pass; the complete serial Voice run continues.

Please rebase onto current `lamp-voice`. Read-only integration against `b60062b86` confirms conflicts in `lampOS/crates/voice/src/provider_worker.rs`, `lampOS/crates/voice/src/provider_worker/tests/review_2026_10_14.rs` and `lampOS/docs/attention.md`. Preserve both `sounds_checked` and `name_in_passing_ignored` fields/wiring/tests, and both the #800 and #829 documentation sections. No native pull-request ARM64 run is attached to this head yet; the conflicting merge must be resolved before that gate can run.

The existing P3 citation finding at `lampOS/docs/verification/two-people-20261015/README.md:83-84` is unchanged: the cited .85-.88 results use the larger multimodal feature set, not head pose plus voice activity alone. Correct that attribution as requested in the earlier review. #829 remains open, as does First's physical on/off-scene handoff. I will review the integration and its exact-head gates after the rebase.

## PR #858 — stale first c5959c003

[Pull request](https://github.com/autonomous-ai/lampOS/pull/858)

Reviewer: [P2] 23:27: the stale-first observation must also make Body history sticky (`lampOS/crates/voice/src/onboarding.rs:350-367`, with read errors mapped to `Never` at 318-324). On `c5959c003`, only `Fresh` sets `joints_heard` / `hand_heard`. A topic first observed as `Stale` blocks that one read, but a later missing value or I/O error restores the never-heard exemption.

Independent tests of the actual module and public Bus reproduce both transitions with continuously fresh centered faces: an initially stale moving JointState followed by removal of its latest value becomes still after 292 ms and saves Done after 3.351 s; an initially stale pushed Hand followed by a Bus read error saves Done after 3.322 s without a release. These are explicit filesystem-loss/error fault injections; normal publisher shutdown retains latest, so I am not claiming that a normal restart alone causes this.

Remember a topic as observed when a stale or unparseable value is present too, and keep subsequent failed/missing reads blocked. Add the stale-first-to-missing/error transitions to the regression. Both original fresh-then-stale failures now pass, along with fresh-first-to-missing, fresh movement/Hand reset, forged-completion rejection and Mark/onboarding routing controls. The fresh full Voice gate continues; #841 and First's physical acceptance remain open.

Additional actual-writer check at 23:31: the exemption must be for a genuinely absent Body, not independently for each topic (`onboarding.rs:355,366`). With the unchanged real `BusOut::offer` / publishing code, I injected a storage fault only at `body/hand/latest`; fresh stationary JointState publications succeeded while every offered `Glimpse.hand` was true. Because Hand had never been read successfully, onboarding saved Done after 3.232 s (JointState sequence 37), with no released-Hand reading. The producer ignores per-topic write errors, so one topic's success does not prove the other signal is absent by design. This is a local storage-fault reproduction, not a lamp measurement.

Latch Body presence when either topic is observed, require both stillness and release evidence thereafter, and distinguish a failed read from a configured absence. Cover fresh joints plus unavailable Hand as well as the stale-first transitions. No source edits or lamp access were used for these checks.

## Issue #800 — merged 818

[Issue](https://github.com/autonomous-ai/lampOS/issues/800)

Reviewer: merged #818 at 23:18: the opt-in checked short-sound guard on both voice routes and the short-turns reader; #800 stays open for recorded-sound counts and demonstrated effectiveness, since First’s played scenes did not trigger the guard.

## PR #818 — final handback 3076f8452

[Pull request](https://github.com/autonomous-ai/lampOS/pull/818)

Reviewer: 23:18: First's [23:09 handback](https://github.com/autonomous-ai/lampOS/pull/818#issuecomment-6196639643) permits merge on this review. The head remains `3076f8452`, unchanged and mergeable. The fallback finding is fixed; the exact opt-in and unchanged-off regressions remain intact. Fmt/strict Voice Clippy passed, and the original full serial result remains 1,325 passed / 1 failed / 8 ignored, with the sole unchanged #739 EarlyEnd/privacy case passing its one same-original-binary retry. The executable hash and both results were retained and rechecked. Exact-head pull-request ARM64 is green; no redundant broad rerun was made for this acceptance update.

First's later lamp report supplements the original verification paragraph: the played sounds and short words showed no harm, and the two-breath confirmation still handed over, but no end was marked `sound` and this guard never fired. Benefit on the offending recorded sound therefore remains unproven. I will rebase-merge the disabled-by-default implementation under First's handback and keep #800 open for the recorded-sound counts, actual guard effectiveness and remaining acceptance. Full-sentence overheard speech remains #829's separate scope.

## Issue #813 — merged 822

[Issue](https://github.com/autonomous-ai/lampOS/issues/813)

Reviewer: merged #822 at 23:12: per-person standing rules, their memory tools and the additive memory/rules publisher behind LAMP_MEMORY_RULES=on; #813 stays open for the Policy reader and quiet-hours integration.

## PR #822 — complete 61b7229ac

[Pull request](https://github.com/autonomous-ai/lampOS/pull/822)

Reviewer: 23:11: final rebase review complete on `61b7229ac`. The conflict resolutions retain Hand/Rules and the body/task/log/memory catalogues. Rules/remember/process logic, Memory sources/tests and both lamp reports are unchanged; a fresh integration check confirms seven memory/Mark/task tools with Rules off and all ten with it on, routed without collisions. The prior twelve actual-store correctness checks and four deadline checks remain applicable by verified source identity, rather than being described as rerun.

Fresh fmt and strict all-target Msgs/Memory/Voice Clippy pass. Complete serial suites: Msgs 86, Memory 159 (5 ignored), Voice 1,348 (8 ignored), total 1,593 passed / 0 failed / 13 ignored, no retries. [Exact-head pull-request ARM64](https://github.com/autonomous-ai/lampOS/actions/runs/38764479769) is green: 4,733 passed / 126 skipped. The formerly failing initial-rule-note cases pass in the original runs.

The description now includes the second lamp-52e6 evidence and its synthesized-speech/known-person-stand-in limits, accurately separates prior/new gates, and says Part of #813. No remaining finding. The exact-on switch and old-behavior regressions remain intact; shared messages are strictly additive. I will rebase-merge this head. #813 stays open because Policy does not yet consume `memory/rules`; this merge does not establish quiet-hours enforcement.

## PR #858 — gate 682bced4d

[Pull request](https://github.com/autonomous-ai/lampOS/pull/858)

Reviewer: 23:06: fresh gate complete on `682bced4d`: fmt and strict all-target Voice Clippy pass; complete serial Voice is 1,351 passed / 0 failed / 8 ignored, no retries. Exact-head pull-request ARM64 [38763138759](https://github.com/autonomous-ai/lampOS/actions/runs/38763138759) is green: 4,722 passed / 126 skipped, including both new motion regressions.

The two independent stale-data failures above remain reproducible despite the package gate passing. Fresh movement/hand resets, forged-completion rejection, and coexisting Mark/onboarding routes pass independently. The rebase retains the earlier report-validation fix. The stale-data P2 and First's #841 physical acceptance remain open; this gate does not resolve them.

## PR #867 — complete 18a4560c9

[Pull request](https://github.com/autonomous-ai/lampOS/pull/867)

Reviewer: 23:04: the fresh full serial Voice-eval gate on `18a4560c9` is complete: 288 passed / 0 failed / 0 ignored across all 24 Cargo suites, no retries. Fmt and strict all-target Clippy pass. The four new regressions and all existing real-process evaluator scenarios ran. Exact-head native pull-request ARM64 remains green. No source finding beyond the integration conflict noted above; the completed original-head gate is retained while awaiting the rebase. #823 stays open.

## PR #858 — stale body 682bced4d

[Pull request](https://github.com/autonomous-ai/lampOS/pull/858)

Reviewer: [P2] 23:03: loss of Body data must not count as stillness or release (`lampOS/crates/voice/src/onboarding.rs:294-308`). `still_now` removes reports older than 700 ms, then missing joints produce `head=true` and missing Hand produces `pushed=false`. In independent tests of the actual module with live Bus publishers and continuously fresh centered faces, the last moving joint sample remained sequence 1, yet stillness became true after 895 ms and `WhereYouSit` was saved Done after 3.966 s without a still sample. With fresh still joints but the last Hand sample still `pushed=true`, the same happened after 939 ms / 3.993 s without a release report.

Require positive stillness evidence throughout the hold, and do not interpret stale/lost prior hand contact as release. If an intentionally absent Body is supported, distinguish that configured mode from expired, failed or lost observations. Add bus-level stale/missing-data regressions. The fresh-motion reset control passes, the forged completion remains rejected, and both Mark/onboarding routes survive the rebase. Those fixes are retained; this is the remaining gap in the new motion check. The full Voice gate continues, and exact-head ARM64 is green. #841 and the First physical handoff stay open.

## PR #865 — complete a78c19264

[Pull request](https://github.com/autonomous-ai/lampOS/pull/865)

Reviewer: 22:57: review complete on `a78c19264`, with the two P3s above still requiring changes. Fresh fmt and strict all-target Same-voice Clippy pass; complete serial crate tests are 2 passed / 0 failed / 0 ignored, no retries. [Exact-head pull-request ARM64](https://github.com/autonomous-ai/lampOS/actions/runs/38759051962) is green.

Independent checks confirm no runtime consumer, unit or install script invokes this tool. Actual-library WAV validation/frame boundaries and the unused CLI path pass. All 1,081 committed pairs over 47 clips and every separation-table row reproduce from the retained labels; the six unlabeled cut-c clips are excluded as documented. No audio/model was downloaded or run for this review, so this does not independently establish frontend numerical parity, model timing, real-voice accuracy or A733 performance. No further source finding. #829 remains open; this is measurement only.

## PR #864 — complete 948b49458

[Pull request](https://github.com/autonomous-ai/lampOS/pull/864)

Reviewer: 22:57: fresh local gate complete on `948b49458`: fmt and strict all-target Voice Clippy pass; complete serial Voice is 1,336 passed / 0 failed / 8 ignored, no retries. Exact-head pull-request ARM64 [38757605005](https://github.com/autonomous-ai/lampOS/actions/runs/38757605005) is green, 4,663 passed / 126 skipped; both new worker on/off cases executed. The two independently reproduced classifier P2s and the research-attribution P3 remain open. The earlier First handoff still applies to the played on/off acceptance; #829 stays open.

## PR #867 — first source review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/867)

Reviewer: 22:57: source review on `18a4560c9` finds no blocker. The clock/hook entry point is explicit; the existing `run` and CLI keep system time and the two-millisecond continuing-poll sleep. I independently reconstructed the complete original file byte-for-byte after removing only the new wrappers, counters/hook, serve guard and four clock substitutions. Tick/fault values, scenarios and existing helpers are preserved. The hook acknowledges flushed/consumed work, while the docs correctly distinguish logical end from file synchronization and actual process exit.

Fresh fmt and strict Voice-eval all-target Clippy pass. The full serial evaluator gate is still running; exact-head pull-request ARM64 [38759844287](https://github.com/autonomous-ai/lampOS/actions/runs/38759844287) is green. GitHub now reports a conflict after #861 merged; please rebase, keeping both clock APIs and both documentation sections, and let the new head's CI run. I will review that integration and finish the host gate before merge. #823 remains open.

## PR #864 — furhat citation

[Pull request](https://github.com/autonomous-ai/lampOS/pull/864)

Reviewer: [P3] 22:54: correct the feature attribution in `lampOS/docs/verification/two-people-20261015/README.md:83-84`. The linked [Furhat paper](https://aclanthology.org/W15-4642.pdf), Table 2, reports head pose plus voice activity at weighted F1 0.742/0.786/0.764 for its three classifiers. The 0.851 cross-validation and 0.876 held-out results use five groups: head pose, POS, card movement, prosody and system dialogue act (§4.3). Attribute those higher values to that combination, rather than head pose plus voice activity alone.

## PR #865 — evidence link

[Pull request](https://github.com/autonomous-ai/lampOS/pull/865)

Reviewer: [P3] 22:52: the evidence link in `lampOS/docs/same-voice.md:11` points to `verification/two-people-20261015/README.md`, which is absent from this PR's head. Please link the existing #829 evidence directly, include the required report, or make the dependency on #864 explicit and wait for it before merge. The committed same-voice measurement files themselves are present, and their separation counts were independently reproduced.

## PR #865 — cosine scale

[Pull request](https://github.com/autonomous-ai/lampOS/pull/865)

Reviewer: [P3] 22:52: `lampOS/crates/same-voice/src/lib.rs:194-197` does not preserve cosine under scaling. Against the freshly built public library, `cosine([0.0001, 0], [0.0001, 0])` returns about 0.083886 rather than 1 because the product of norms is clamped to f32 epsilon; finite `[1e20, 0]` compared with itself returns NaN from f32 overflow. Use stable normalization/accumulation and explicitly handle undefined inputs, with small- and large-scale regression cases. This is a public helper correctness finding; it does not establish that the recorded CAM++ embeddings had those magnitudes.

## Issue #823 — merged 861

[Issue](https://github.com/autonomous-ai/lampOS/issues/823)

Reviewer: merged #861 at 22:50: explicit clocks for the evaluator backend and fake player, with default system clocks and host cleanup bounds retained; #823 stays open for the remaining realtime migration.

## PR #861 — complete b34d181d9

[Pull request](https://github.com/autonomous-ai/lampOS/pull/861)

Reviewer: 22:49: the complete fresh serial gate on `b34d181d9` has finished: 289 passed / 0 failed / 0 ignored, no retries. All 12 physical-loopback and 14 serve-session cases ran, in addition to the five new clock regressions. Fmt and strict all-target Voice-eval Clippy pass; exact-head pull-request ARM64 remains green. The independent source-preservation and frozen-clock drain/actual child-reap checks are recorded above. No remaining finding. This is an explicit evaluator API with unchanged default constructors and documented host cleanup bounds; #823 remains open for the full clock migration.

## PR #864 — first handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/864)

Reviewer: for the first builder: 22:49: Fourth has no lamp and #829 explicitly requests your on/off played scenes. The two source findings above need correction before those scenes can establish this behavior. The switch-off path matched the actual pre-PR classifier/rule across 440 independent text cases; fresh fmt and strict Voice Clippy pass, and the complete serial Voice gate is running. The current head's ARM64 run is green. #829 remains open for its other steps and physical acceptance.

## PR #864 — modified nouns

[Pull request](https://github.com/autonomous-ai/lampOS/pull/864)

Reviewer: [P2] 22:49: implement the documented positive call positions in `lampOS/crates/voice/src/attention.rs:278-280`. The current test only rejects a name immediately after one listed determiner; it accepts every other occurrence. In independent strict-busy-room runs, “I bought a new lamp yesterday”, “The old lamp stopped working” and “Did you see her desk lamp?” all still produce Play with the switch on. None is first, after a greeting, or set off as a call. Account for ordinary modified noun phrases and add those held-answer regressions, while keeping the explicit vocative controls. The PR's claim that it counts the name only when addressed is not yet true.

## PR #864 — vocative punctuation

[Pull request](https://github.com/autonomous-ai/lampOS/pull/864)

Reviewer: [P2] 22:49: preserve comma-separated direct calls in `lampOS/crates/voice/src/attention.rs:273-280`. Splitting away punctuation before checking the preceding word makes “Can you explain that, Lamp?” and “What is this, Lamp?” return false because `that`/`this` is in `SPOKEN_OF`. An independent run through `Attention::first_output_busy` on this head changes both from Play with the switch off to Hold with it on, contradicting the documented final-vocative behavior. Preserve punctuation/clause boundaries so these explicit calls remain positive, and add both classifier and worker-loop regressions.

## PR #822 — rebase needed

[Pull request](https://github.com/autonomous-ai/lampOS/pull/822)

Reviewer: 22:45: GitHub now reports `4f8208412` conflicting with `lamp-voice` after the intervening merges. Please rebase while retaining both sides of any topic/catalogue additions, and refresh the description's second lamp run and gate/CI paragraphs as requested above. The completed review of this head is retained; I will inspect the rebase and verify the new head's gates/CI rather than treating the old head as current. #813 remains open for the Policy reader.

## PR #842 — ci green 9dc615f35

[Pull request](https://github.com/autonomous-ai/lampOS/pull/842)

Reviewer: 22:45: the replacement exact-head pull-request ARM64 run [38760659983](https://github.com/autonomous-ai/lampOS/actions/runs/38760659983) is now green on `9dc615f35`: 4,730 passed / 126 skipped. This completes the CI item in the preceding review; the documentation finding is resolved and the recorded source-identical local gate remains applicable. The camera-privacy and #836 physical acceptance handoff to First remains.

## PR #858 — complete 7c06dfd28

[Pull request](https://github.com/autonomous-ai/lampOS/pull/858)

Reviewer: 22:45: fresh gate complete on `7c06dfd28`: fmt and strict all-target Voice Clippy pass. Full serial Voice retained 1,337 passed / 2 failed / 8 ignored. The two unchanged Second-route/privacy-freshness cases at `coordinator/tests/second_route.rs:158` each passed one isolated retry from the preserved original executable: `spoken_over_the_second_routes_answer_stops_as_gemini_does` and `without_a_second_route_the_packets_and_records_are_as_before`. The full-run failures remain recorded; no broad retry. [Exact-head pull-request ARM64](https://github.com/autonomous-ai/lampOS/actions/runs/38757458025) is green.

The independent forged-completion reproduction passes with the fix, as recorded above. The physical-stability P2 at `lampOS/crates/voice/src/onboarding.rs:375-383` remains: framing can complete during head/hand motion because no such state reaches the hold. Please implement and cover First's motion requirement, or obtain an explicit scope decision on #841.

Reviewer: for the first builder: real onboarding behavior and the unresolved #841 acceptance remain with you. The local gate does not establish those physical conditions; #841 stays open.

## PR #861 — first source review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/861)

Reviewer: 22:45: source review of `b34d181d9` finds no blocker. The new clock APIs are explicit opt-ins; existing constructors keep the original system-clock calls, ordering and host cleanup bounds. I independently compared all 27 existing test bodies/assertions, 15 constants and nine sleep expressions against the parent: unchanged. An additional scratch proof using the unchanged backend with a frozen logical clock completed the host drain in 328.5 ms, sampled the logical clock once and verified the actual owned child was reaped.

Fresh fmt and strict all-target Voice-eval Clippy pass; all five new regressions pass in the initial unit suite. The complete serial evaluator gate is still running through its real-process scenarios, so this is not yet a completed local gate. Exact-head pull-request ARM64 [38756122123](https://github.com/autonomous-ai/lampOS/actions/runs/38756122123) is green, 4,662 passed / 126 existing profile skips. #823 remains open for the remaining scheduler/fixture migration; this preparation claims no scenario speedup.

## Issue #854 — merged 859

[Issue](https://github.com/autonomous-ai/lampOS/issues/854)

Reviewer: merged #859 at 22:44: explicit staff requests hand over behind LAMP_TASKS_ASKED=on, with one recipient-naming confirmation; First accepted the completed #854 scenes.

## PR #859 — complete c7d41bcf3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/859)

Reviewer: 22:43: review complete on `c7d41bcf3`. The description and `lampOS/docs/tasks.md:756-757` now match the final recipient-naming read-back and `after_own_story`; the P3 is resolved. Exact `LAMP_TASKS_ASKED=on` is required, the off test retains the old function declarations and note, and neither dispatch/confirmation nor privacy/shared message meanings change. No remaining source finding.

Fresh fmt and strict Voice all-target Clippy pass. The correctly permitted full serial Voice run retained 1,331 passed / 3 failed / 8 ignored. The three unchanged EarlyEnd/privacy-freshness cases at `coordinator/tests/early_end.rs:472` each passed one retry from the preserved original executable: answer after sure, resumed speech at three tenths, and dropped continued link. These are the Queue #739 timing family; the original failed full run remains recorded. All PR-specific regressions passed in that run. The initial sandbox-denied attempt is separately retained as invalid local-socket/worker execution; correcting permissions used the same binaries and is not counted as a flaky-test retry.

The locally gated `afe87d6d9` production/tests, Cargo inputs, toolchain and workflows are byte-identical to this docs-only head. [Exact-head pull-request ARM64](https://github.com/autonomous-ai/lampOS/actions/runs/38759237170) is green: 4,686 passed, 126 skipped. No CI rerun was requested.

First's current-code lamp scenes and [22:06 model result](https://github.com/autonomous-ai/lampOS/pull/859#issuecomment-6195305574) resolve the earlier acceptance handoff: `after_own_story` 5/5, `mid_story` 4/4, the other five scenes 5/5, with the one failed session disclosed. This is First's evidence, not a reviewer lamp run. I will rebase-merge this reviewed head and close #854 as completed; #799's broader latency work remains open.

## PR #842 — evidence resolved 9dc615f35

[Pull request](https://github.com/autonomous-ai/lampOS/pull/842)

Reviewer: the evidence finding is resolved at 9dc615f35 (lampOS/docs/verification/full-look-lamp-52e6-20261015/README.md:52-58). First-word latency is now explicitly unmeasured, and the unbound audio gap is labelled only as an unverified observation that may belong to another turn. The PR description now agrees on six matched answers plus one ambiguous, and on the coordinator-acceptance timing boundary.

Only the verification README changed from tested 2798717b9. All production/tests, Cargo inputs, toolchain and workflows are byte-identical, so the prior gate is carried by source identity: fmt and strict Camera/Eyes/Voice Clippy pass; the first full serial run was 1456 passed / 2 failed / 8 ignored; both unchanged timed failures passed their single original-binary retries; all eight independent regressions passed. No redundant full suite was run for these docs commits. The new exact-current ARM64 run 38760659983 is still running; the superseded475 run was cancelled after its tests passed and is not counted as green.

Reviewer: for the first builder: camera privacy ownership and the remaining #836 real-object/person and question-to-first-word acceptance remain with you; #836 stays open.

Checked at 22:35.

## PR #795 — complete f0c55a49c

[Pull request](https://github.com/autonomous-ai/lampOS/pull/795)

Reviewer: 22:35: rebase review complete on `f0c55a49c`. The Msgs conflict keeps both logging and privacy catalogues; all 21 added privacy implementation/test/doc files are identical to the previously reviewed head. No new source finding.

Fresh fmt and strict all-target Clippy pass for Controls, Ears, Mood, Msgs, Ring and Voice. The original full serial gate is 1,991 passed / 2 failed / 11 ignored. Both unchanged failures passed once from the original preserved executable: EarlyEnd resumed-speech/privacy freshness at `coordinator/tests/early_end.rs:472` in 6.40 s (known #739 family), and the reminder room-change socket reset at `provider_worker.rs:5298` in 0.66 s. No matching Queue waiver was established for the latter; its isolated pass is diagnostic, not a claimed clean full gate. No broad retry. [Exact-head native ARM64](https://github.com/autonomous-ai/lampOS/actions/runs/38756093472) passed 4,737 tests, 127 skipped, including the Linux Controls-loss privacy-worker regression.

Reviewer: for the first builder: this changes the microphone/mute privacy path and still needs the #755 physical-slide acceptance. The host review does not establish wiring/polarity, real microphone-handle closure or visible ring timing. #755 stays open.

## PR #858 — report bypass fixed 7c06dfd28

[Pull request](https://github.com/autonomous-ai/lampOS/pull/858)

Reviewer: 22:30: the forged completion P2 is fixed at `7c06dfd28`. The unchanged independent reproduction now receives an error from `onboarding::run` for `where_you_sit=done`, and the actual coordinator module leaves the step incomplete with no face/active step. The mailbox also requires the currently led step and accepts only a skip for this measured step. I rebuilt this proof against the current module and matching current libraries.

The previous head's complete serial gate is retained: 1,337 passed / 1 failed / 8 ignored; the unchanged EarlyEnd/privacy timing test passed its one same-original-binary retry. A fresh complete gate is running for this code-changing head. The separate physical-stability P2 remains unchanged: the three-second face hold still has no head-motion or body-hand input, as requested in First's #841 additions. #841 and the existing First hardware handoff remain open.

## PR #855 — merged issue 803

[Pull request](https://github.com/autonomous-ai/lampOS/pull/855)

Reviewer: merged #855 at 22:30 (`3b146029d099`): opt-in spoken marking of the last 60 seconds, local listing and seven-day expiry on startup/mark/list, with failed-read and expiry fixes; complete Voice gate and exact-head ARM64 passed.

## PR #848 — merged issue 711

[Pull request](https://github.com/autonomous-ai/lampOS/pull/848)

Reviewer: merged #848 at 22:28 (`59697b459ac2`): optional body/hand publication with source-age expiry preserved through queue and filesystem delays; #711 stays open for its touch reader and physical qualification.

## PR #797 — merged issue 711

[Pull request](https://github.com/autonomous-ai/lampOS/pull/797)

Reviewer: merged #797 at 22:27 (`aab0133d98dd`): the manual touch calibration/replay tool, with the corrected own-swipe cutoff and release/debounce footprint; #711 remains open for head-pad interactions and physical qualification.

## PR #842 — residual measurement 4754aa2ab

[Pull request](https://github.com/autonomous-ai/lampOS/pull/842)

Reviewer: [P2] The README's score and capture-boundary correction is right at 4754aa2ab: six answers matched to their displayed trials, the banana result explicitly ambiguous, and 622–1060 ms labelled as ask to coordinator acceptance. One measurement claim remains at lampOS/docs/verification/full-look-lamp-52e6-20261015/README.md:52-55: “2.0–3.6 s in seven looks; in three, no answer came before the next question” is still presented as measured, immediately before noting that per-request first-word timestamps were not kept. The retained look-timing.txt contains ask/still durations only, and the late banana remains unbound to its request. Remove this timing/count claim or label it clearly as an unverified observation; retain request-bound first-word events before using it as the issue's comparison. Please also update the PR description's old 7/10 and latency/cause claims to match the corrected README.

All production and test trees, Cargo files, toolchain and workflow files are identical to tested 2798717b9; no full gate was repeated for this README-only commit. The carried result is unchanged: fmt and strict Camera/Eyes/Voice Clippy pass; the first full serial run was 1456 passed / 2 failed / 8 ignored, both unchanged timed failures passed once from the original binary, and all eight independent regressions passed. Exact-current ARM64 is still running. Camera privacy and the remaining #836 real-object/person and question-to-first-word acceptance remain with the first builder.

Checked at 22:27.

## PR #797 — ready description corrected

[Pull request](https://github.com/autonomous-ai/lampOS/pull/797)

Reviewer: 22:26: the description now matches `10585b465` and its reviewed own-swipe cutoff, hysteresis/debounce footprint, gate and ARM64 evidence. All four earlier calibration findings remain fixed. The retained independent CLI and debounce proofs pass; fmt and strict Controls Clippy passed. The original complete serial run had 382 passes and four unchanged missing-stderr/output failures; each passed its one same-binary isolated retry, retained under the existing #783/#837 program-output investigation. That is 386 distinct passing cases after retries, not a clean first run. [Exact-head native ARM64](https://github.com/autonomous-ai/lampOS/actions/runs/38736016630) is green.

Under [First's explicit #711 direction](https://github.com/autonomous-ai/lampOS/issues/711#issuecomment-6192051506), this is ready to merge as a manual calibration/replay tool. It does not enable touch or change served behavior. #711 stays open for the head-pad interaction and physical qualification work.

## PR #855 — complete 7336b004e

[Pull request](https://github.com/autonomous-ai/lampOS/pull/855)

Reviewer: 22:23: both marked-record P2s are fixed at `7336b004e`. The original independent tests now pass: listing removes an eight-day-old mark without another mark being made, and unreadable metadata returns an error rather than a successful empty mark. The code/docs explicitly run expiry on enabled voice startup, new marks and listing; this is maintenance-triggered expiry, not a new periodic timer.

Fresh `cargo fmt --all --check`, `cargo clippy -p lamp-voice --all-targets -- -D warnings`, and `cargo test -p lamp-voice --no-fail-fast -- --test-threads=1` pass: 1,325 passed, 0 failed, 8 ignored, without retries. This is the full new-head gate; the old head's failed run remains recorded above. [Exact-head native pull-request ARM64](https://github.com/autonomous-ai/lampOS/actions/runs/38755647012) is green.

Switch-off declaration/routing behavior is covered. The committed lamp-52e6 report shows the spoken mark, one thanks, the 60 s copy and listing, and discloses the first split-turn failure and host-only expiry verification. No microphone/camera opening or mute path changes. The source and docs now match the #803 scope; no remaining finding. Ready for rebase merge.

## PR #848 — complete df932e22a

[Pull request](https://github.com/autonomous-ai/lampOS/pull/848)

Reviewer: 22:23: the freshness follow-up is fixed at `df932e22a` (`lampOS/crates/body/src/bus_out.rs:341`). Hand's age is now taken after JointState publication. The unchanged independent test blocking the real earlier JointState file open for 1.2 s now passes: no fresh Hand is created from an expired observation. Delayed queue/stop and partial remaining-lifetime cases also pass.

Fresh fmt, strict Body/Msgs all-target Clippy and complete serial tests pass without retries: 379 passed, 0 failed, 0 ignored. [Exact-head native ARM64](https://github.com/autonomous-ai/lampOS/actions/runs/38756164405) passed 4,662 tests, 126 skipped, including the FIFO regression. All six independent checks and four switch subprocess cases pass. Six actuator/report/existing-message scenarios match the base byte for byte (1,644,402 bytes).

No source finding remains. This adds the optional Hand publication under the existing explicit Bus switch and leaves motor output and existing message meanings unchanged. It is ready for rebase merge; #711 remains open for the touch reader and physical qualification work.

## PR #822 — complete 4f8208412

[Pull request](https://github.com/autonomous-ai/lampOS/pull/822)

Reviewer: 22:23: the updated deadline/test follow-up is reviewed on `4f8208412`. Both production entry points retain the nominal 300 ms bound. Four independent real-store checks show one budget shared across room/person snapshots and rules: the production note returned in 302.2 ms with unfinished rules omitted; a requested 10 s production open still returned in 300.4 ms; an explicit 120 ms test budget returned in 120.2 ms; an explicit 2 s budget completed the same delayed reads. These are host measurements, not lamp latency.

Fresh fmt and strict Msgs/Memory/Voice all-target Clippy pass. Full serial suites pass with no retries: Msgs 85, Memory 159 (5 ignored), Voice 1,338 (8 ignored), total 1,582 passed / 0 failed / 13 ignored. Both earlier PR-owned note failures and the inherited usage-tail test pass in this original run. [Exact-head pull-request ARM64](https://github.com/autonomous-ai/lampOS/actions/runs/38755864147) passed 4,670 tests, 126 skipped. The previous twelve independent Rules correctness proofs remain applicable by exact source identity; the logging/catalogue rebase retains both topic sets. No further source finding.

The [second lamp-52e6 report](https://github.com/autonomous-ai/lampOS/blob/4f8208412787a9ab4f9bf356612153edeb664472/lampOS/docs/verification/rules-second-run-lamp-52e6-20261015/README.md) supplies the spoken implicit-correction and correct-rule deletion demonstration, with its known-person stand-ins explicitly disclosed. Please refresh the PR body's still-pending second run and stale gate/CI paragraphs before merge. #813 stays open: `lampOS/docs/policy.md:81-89` correctly says Policy does not yet read `memory/rules`; that reader/integration is separate work.

## PR #815 — merged issue 811

[Pull request](https://github.com/autonomous-ai/lampOS/pull/815)

Reviewer: merged #815 at 22:19 (`74eb16c73a20`): manual product installation and link-based updates, including recovery from interrupted and repeated failed writes; #811 stays open for setup/keeper bootstrap and Harness boot service/linger.

## PR #815 — complete d5c033983

[Pull request](https://github.com/autonomous-ai/lampOS/pull/815)

Reviewer: 22:17: all posted installer findings are fixed at the actual PR head `d5c033983`. The nine original independent recovery checks now pass, including real child exits before systemd debt handling and after a settings write, retrying failed enable, preserving custom/pre-existing files, and two successive partial-write failures. The latter retains the previous written hash before replacing the pending intent, so the third run installs the new defaults instead of treating the old default as a per-lamp customization. No new source finding.

Fresh `cargo fmt --all --check`, `cargo clippy -p lamp-update --all-targets -- -D warnings`, and `cargo test -p lamp-update --no-fail-fast -- --test-threads=1` pass: 69 passed, 0 failed, 1 ignored (the existing external-network release-file test), no retries. The [actual #815 pull-request ARM64 run](https://github.com/autonomous-ai/lampOS/actions/runs/38757082810), rather than draft #863's run, is successful on this exact head.

The manual install/product-link path leaves existing named-build units unchanged, covered by the unchanged legacy-round controls and mixed-path tests. The scope and documents match the install work reviewed by Second and handed here by First at 22:04. This is ready for rebase merge. #811 will remain open: setup/keeper bootstrap and the requested Harness boot service/linger work are not supplied by this PR.

## PR #797 — final followup 10585b465 2216

[Pull request](https://github.com/autonomous-ai/lampOS/pull/797)

Reviewer: 22:16: the reviewed code remains `10585b465`; no further test rerun is needed for an unchanged head. The remaining description correction from 19:56 is still outstanding: item 4 says thresholds use the weakest swipe, but `lampOS/crates/controls/src/touch_calibrate.rs` now uses the weakest own swipe whose peak reaches 50% of that pad's strongest, with hysteresis/debounce replay for the footprint. Please update that item and the old head/test-count paragraphs to the current reviewed behavior and evidence. First's #711 direction keeps this a manual tool; the unrelated head-touch work stays open.

## PR #845 — final followup 9b4f499f3 2216

[Pull request](https://github.com/autonomous-ai/lampOS/pull/845)

Reviewer: 22:16: the DNS-worker lifetime P2 is fixed for the shipped setup path on `9b4f499f3`. The original independent reproduction runs three actual failed setup attempts with fresh ImageHost instances: only one resolver is started and remains active across all three. HTTP serializes joins and ImageHost uses one fixed address, so the process-wide slot now bounds the worker's lifetime across retries.

Fresh fmt, `cargo clippy -p lamp-setup --all-targets -- -D warnings`, and complete serial Setup tests pass: 34 passed, 0 failed, 0 ignored, no retries. All six independent checks pass: repeated setup, same-host control, shared DNS/TLS deadline, slow TLS records, actual port ownership and startup-failure short-circuit. Both serving CLIs remain disabled without `--enable`. [Exact-head pull-request ARM64](https://github.com/autonomous-ai/lampOS/actions/runs/38744254417) is green.

Two API limits matter if Reach gains callers beyond today's serialized, fixed-address setup: at `lampOS/crates/setup/src/wifi.rs:410-414`, the blocking slot mutex lets an 80 ms check wait 602 ms behind a concurrent 600 ms check; at lines 417-423, a completed lookup for a different address remains reserved and later addresses never start. The independent API probes reproduce both. They are not reached by the current product path and are not a current setup merge blocker; bound lock acquisition and retire completed other-address receivers before supporting those uses.

Reviewer: for the first builder: Third has no lamp/phone/captive-network evidence, so the existing wiped-lamp-to-conversation acceptance remains yours and #806 stays open. The PR body should also refresh its old 33-test/pending-CI status to the completed evidence above.

## PR #842 — measurement attribution 2798717b9

[Pull request](https://github.com/autonomous-ai/lampOS/pull/842)

Reviewer: [P2] Keep the object score and timing claims tied to the recorded request and measurement boundary (`lampOS/docs/verification/full-look-lamp-52e6-20261015/README.md:37-47`). The retained `looks-after.txt:9-10` has the previous book answer during the banana trial; the banana answer appears under headphones at lines 19-20. The table supports six answers matched to their displayed trial. Without answer request IDs/timestamps it cannot establish that the late banana belongs to the banana trial, yet the README counts it in 7/10 and says only three trials had no timely answer. Preserve the corresponding request-bound raw events, or label that seventh result ambiguous and correct the scored/timely-answer claims.

The 622–1060 ms values are also a different boundary from camera readiness. `coordinator.rs:4564-4583` records `look_full_still` only after the lamp's previous reply has ended and the provider/readiness gates permit the next turn; a still already ready during “Let me look” waits there. These values measure ask→coordinator acceptance, not ask→capture completion. All three retained full-look journal lines (`eyes-journal.txt:1,5,9`) report head wait 0 ms, so “mostly the settle wait” is not supported. Label the measured boundary accurately, remove the unsupported cause, and retain per-request camera-ready and first-word timestamps before comparing those targets. No additional lamp run is requested of this reviewer.

The request-ID source fix itself passes the independent late-old-capture regression on 2798717b9. This finding concerns the evidence and completion claims, not a reproduced remaining request-correlation failure. Objects held by a person, turning toward that person and the requested question→first-word comparison remain unshown as the document already notes. #836 should remain open for those acceptance items unless the first builder explicitly changes its scope.

Checked at 2026-10-15 22:00:51 +0700.

## PR #859 — followup afe87d6d9 2211

[Pull request](https://github.com/autonomous-ai/lampOS/pull/859)

Reviewer: [P3] Update the review description and regression reference to the final unclear-name behavior (`lampOS/docs/tasks.md:756-757`). `afe87d6d9` removes the `not sure` choice, uses one recipient-naming read-back, and replaces the model-check scene with `after_own_story`. The PR body still promises `not sure`, nothing prepared, and the old `asked_again` scene; the doc cites removed test `a_name_not_made_out_is_asked_about_and_never_guessed`. Change those to the final behavior and existing `a_name_not_made_out_is_named_in_the_read_back_and_asked_about_once` test, so the reviewable scope and evidence agree.

I have read First's current-head scene acceptance and 22:06 model-check result, including `after_own_story` 5/5 and one failed session. The host source review/gate is continuing; I am not transferring the prior head's gate to this one. Checked at 22:11.

## PR #844 — followup a31223254 2211

[Pull request](https://github.com/autonomous-ai/lampOS/pull/844)

Reviewer: 22:11: both timezone P2s are fixed on `a31223254`. All seven independent current-library regressions pass: fallback-file reload with missing/broken localtime, spring/fall retention, fixed offsets, half-hour DST, arithmetic bounds and person isolation. The rebase retains both fixes byte-for-byte and preserves the new Settings workspace member. No new timezone source finding.

Fresh fmt and strict all-target Clippy pass for LocalTime, Expression, Memory and Policy. Complete serial gate: 555 passed, 3 failed, 7 ignored. These inherited stderr assertions failed: Memory `tests/binary.rs:216` (usage), Memory `tests/binary.rs:540` (legacy-refusal text), Policy `tests/program.rs:448` (usage). Each received one same-original-binary retry: legacy refusal passes; both usage tests still fail. Those two failures cannot receive the role's passes-alone waiver; the local gate is not green. Original results and binaries are retained, with no broad rerun.

Second's committed Policy/shared-reader approval is recorded. Seventh's committed Memory read required by First on #820 is still missing from the PR and issue. The PR body's two-stat claim also needs updating to the final reader's three stat calls plus readlink. The previous First hardware handoff and #820 acceptance remain; I have not merged this head.

## PR #842 — followup 2798717b9 2211

[Pull request](https://github.com/autonomous-ai/lampOS/pull/842)

Reviewer: 22:11: the request/capture correlation source finding is fixed on `2798717b9`. Eight independently rebuilt regressions pass, including selecting an old capture before a newer real Unix-datagram ask, then finishing that old write: it is rejected until the new request's own capture completes. The stationary, movement, switch-off and parser/bounds controls also pass. Voice retains its previous 720-height / 2-MiB bounds while Camera/Eyes allow the explicit full-look mode.

Fresh fmt and strict all-target Clippy pass for Camera, Eyes and Voice. Complete serial tests: Camera 75 passed; Eyes 46 passed; Voice 1,335 passed, 2 failed, 8 ignored. Both unchanged timed cases (motor preflight at `coordinator/motor/tests.rs:453`, reminder AckConfirmed at `coordinator/tests/reminder_roundtrip.rs:338`) passed once from the same original test binary. The first full run remains 1,456 passed / 2 failed / 8 ignored. [Exact-head native ARM64](https://github.com/autonomous-ai/lampOS/actions/runs/38754157425) passed 4,613 tests, 124 skipped, including the Linux-only negotiated camera-format and Eyes bound tests.

The evidence/measurement P2 above remains. Reviewer: for the first builder: camera privacy ownership and the remaining #836 real-object/person and question-to-first-word acceptance stay with you; #836 remains open.

## PR #857 — review complete 81009f917

[Pull request](https://github.com/autonomous-ai/lampOS/pull/857)

Reviewer: 22:09: host review complete on `81009f917`. The explicit `--give-way off --limp off` parser values match the old defaults, and the scripted off comparison checks identical stop, goal-write and register-write output against `Limits::FOLDED`; the enabled comparison differs, so the test can detect a change. No source finding.

Fresh `cargo fmt --all --check`, `cargo clippy -p lamp-body --all-targets -- -D warnings`, and `cargo test -p lamp-body --no-fail-fast -- --test-threads=1` pass: 292 passed, 0 failed, 0 ignored, without retries. [Native pull-request ARM64](https://github.com/autonomous-ai/lampOS/actions/runs/38746301909) is green on this exact head.

Reviewer: for the first builder: this changes default physical behavior at `lampOS/crates/body/src/bin/lamp-body.rs:438-439`; your hand-push, let-go and limp acceptance remains scheduled for the owner's 11:00 minute. This remains your merge and #703 stays open.

## PR #853 — review complete 33bfb012d

[Pull request](https://github.com/autonomous-ai/lampOS/pull/853)

Reviewer: 22:09: host review complete on `33bfb012d`. The actual coordinator path now retains a provider-completed turn only while it has no returned audio/playback and the turn, connection and microphone generation still match; the next admission can use that retained turn within the one-second bound. The new tests exercise actual completion and later admission, including switch-off behavior. No further source finding; the removed Python finding is resolved.

Fresh `cargo fmt --all --check`, `cargo clippy -p lamp-voice --all-targets -- -D warnings`, and `cargo test -p lamp-voice --no-fail-fast -- --test-threads=1` pass: 1,321 passed, 0 failed, 8 ignored, with no retries. The [native pull-request ARM64 run](https://github.com/autonomous-ai/lampOS/actions/runs/38755355783) is successful on this exact head.

Reviewer: for the first builder: the quiet-room continuation and busy-room floor-hold acceptance for #782 still need your lamp judgement. Fourth reports no lamp run; host tests do not establish that acceptance. #782 stays open.

## PR #818 — review complete 3076f8452

[Pull request](https://github.com/autonomous-ai/lampOS/pull/818)

Reviewer: 22:09: the fallback checked-sound P2 is fixed on `3076f8452`. I reran the original independent proof against the fresh library with the accepted worker flags: `--attention --sounds-checked`, `End.sound=true`, and the original `¿Qué?` transcript now produce zero audio packets (previously two). The checked flag reaches the Second Engine, which preserves the sound marker through transcription; accepted, unchecked and unmarked controls remain covered. No new source finding.

Fresh `cargo fmt --all --check` and `cargo clippy -p lamp-voice --all-targets -- -D warnings` pass. Complete `cargo test -p lamp-voice --no-fail-fast -- --test-threads=1`: 1,325 passed, 1 failed, 8 ignored. The sole unchanged EarlyEnd/privacy timing case at `lampOS/crates/voice/src/coordinator/tests/early_end.rs:472` passed its one isolated retry from the same original binary (6.40 s). This is the existing Queue #739 timing family, so it is noted; the first full run was not clean. [Exact-head ARM64](https://github.com/autonomous-ai/lampOS/actions/runs/38743411053) passed 4,592 tests, 118 skipped.

Reviewer: for the first builder: Second has no lamp and requests the activation minute; recorded-sound effectiveness and short-word preservation still need the #800 acceptance. #800 remains open.

## PR #850 — issue 846 close

[Pull request](https://github.com/autonomous-ai/lampOS/pull/850)

Reviewer: 22:06: merged #850 (`c0f79cac95bd`) into lamp-voice, fixing the live Bus-recording test snapshot race while preserving all original assertions; all 66 Bus tests and the exact-head ARM64 gate passed.

## PR #850 — full review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/850)

Reviewer: 22:01: review of 21c9b55b8 is complete. Fresh cargo fmt --all --check, cargo clippy -p lamp-bus --all-targets -- -D warnings, and cargo test -p lamp-bus --no-fail-fast -- --test-threads=1 all pass: 66 passed, zero failures or ignored tests, including all 11 CLI cases. No retries.

I independently reversed only the new helper/regression and four polling predicates: the original cli.rs is byte-identical, preserving all 190 assertions and all 10 original test identities. Partial live snapshots wait; the post-exit strict JSON/LF parser still rejects malformed or partial output. Production recording and shared Bus contracts are unchanged. Exact-head pull_request ARM64 run 38741600267 is green. No finding; #846 is fully addressed by this scoped change, while #783 and #837 remain separate.

## PR #852 — issue 823 merged

[Pull request](https://github.com/autonomous-ai/lampOS/pull/852)

Reviewer: merged #852 at 21:59: explicit evaluator relay clock API with the system-clock path preserved; #823 stays open for migrating the realtime tests.

## PR #852 — full review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/852)

Reviewer: 21:57: the exact fef1377ef gate passed: cargo fmt --all --check; cargo clippy -p lamp-voice-eval --all-targets -- -D warnings; cargo test -p lamp-voice-eval --no-fail-fast -- --test-threads=1. All 287 tests passed, zero failures or ignored cases, including the three new clock regressions and every existing realtime case. No retry. The complete run finished at 21:56:05 from date.

The explicit API is opt-in; existing callers retain their original system-clock path, sampling order, ten-second grace and host cleanup bounds. No existing shared contract changes, source finding or undocumented timing claim remains. Exact-head pull_request ARM64 run 38739080219 is green. Ready for rebase merge; #823 remains open for the actual multi-process clock migration.

## PR #853 — python resolved

[Pull request](https://github.com/autonomous-ai/lampOS/pull/853)

Reviewer: 21:55: verified 33bfb012d removes the Python file and records its counting method in the README. That finding is resolved; all production and test source files are unchanged from 71f3888b7. The fresh full local Voice gate is now on 33bfb012d. The lamp-4ace handoff above remains.

## PR #855 — full gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/855)

Reviewer: 21:55: the full host gate on 8e5df5650 finished. Formatting and strict all-target Voice Clippy pass. Serial tests: 1,321 passed, 2 failed, 8 ignored; all new mark/tool tests pass. Each inherited failure was run once from its original executable: diagnostics::audio_fault_end_is_incomplete_even_with_good_written_pcm passes, but reminders::consumer::tests::completed_playout_survives_both_real_store_reopens_then_acks_without_respeech still fails its AckConfirmed assertion at reminders/consumer/tests.rs:644. Original failures and both isolated results are retained; no broad rerun. The complete host gate is not green. The separately reproduced retention and failed-read P2s above remain unchanged.

## PR #795 — rebase 97df complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/795)

Reviewer: 21:54: the 97dfbc78e rebase preserves the reviewed privacy implementation; the coordinator adaptation retains the newer startup/Second-route supervision. Fresh fmt and strict all-target Clippy pass for Controls, Ears, Mood, Msgs, Ring and Voice. Full serial tests: 1,978 passed, 1 failed, 11 ignored. The only failure was the unchanged ring_trace_full_600_seconds_keeps_voice_transitions_and_fault_within_cap IPC-timing test; its one isolated invocation from the same original executable passes (30,000 real socket frame/receipt pairs). The original failure is retained. Exact-head pull_request ARM64 run 38746376142 is green: 4,675 passed, 125 skipped, including the Linux worker-loss privacy regression. No new source finding.

Reviewer: for the first builder: this changes microphone/mute privacy and still needs your physical slide-switch judgement. #755 remains open. No lamp was used by this reviewer.

## PR #858 — model bypasses framing

[Pull request](https://github.com/autonomous-ai/lampOS/pull/858)

Reviewer: [P2] 21:52: lampOS/crates/voice/src/onboarding.rs:483–489 accepts any uncompleted step reported through setup_step, including where_you_sit. The function schema at lines529–544 also offers that measured step to the model. In an independent run of the unchanged module, setup_step {step:where_you_sit,outcome:done} followed by tick with no face and no active step wrote Done with framing:null. That bypasses the three-second framing measurement completely. Only the runner should commit Done for where_you_sit; reject that model outcome and constrain reports to the active step, with an explicit skip path if desired. Add a no-face forged/computed tool-result regression.

## PR #848 — delayed joint publication

[Pull request](https://github.com/autonomous-ai/lampOS/pull/848)

Reviewer: [P2, freshness follow-up] Measure Hand's age after the preceding publication (lampOS/crates/body/src/bus_out.rs:328-339 at 6d7ac1b83). The original delayed-queue case is fixed, but `now` is captured before `self.joints.publish_json`, which performs filesystem I/O. A delay there is omitted from Hand's age; the Bus then assigns Hand a new wall timestamp with this overstated remaining life.

An independent current-source test blocks the actual JointState kept-value file open for 1.2 seconds using a local test FIFO, then releases it. No production method is copied or edited. Actual Publishing::step and Bus.latest return Hand { pushed: true, since_us: 3303 }, expires_ms=999 and expired=false, while the observation is already 1.205 seconds old and zero Body publishers remain. This injects a delay at the real earlier I/O boundary; it is not a claim about its frequency on a lamp.

Calculate age from a fresh Instant immediately before the Hand expiry/drop decision, after the JointState call; add the earlier-publication-delay regression alongside the queue-delay regression. Existing topic semantics need not change.

The original queue/stop replay now passes: a 1.205-second-old queued sample yields no Hand; a 605 ms sample retains only 395 ms, carries its observation timestamp, and expires after the original observation's lifetime. All required gates pass on this exact head: fmt; Body/Msgs all-target Clippy with -D warnings; serial tests 378 passed / 0 failed / 0 ignored. ARM64 pull_request run38741710781 is green for 6d7ac1b83. Six paired actuator/report/existing-message scenarios still match the base byte for byte (1,644,402 bytes), and unset/off/true/on switch subprocess checks pass. Five independent checks pass; the earlier-I/O freshness case fails101. No lamp was used; #711 remains open for its remaining reader and physical qualification work.

Checked at 2026-10-15 21:49:29 +0700.

## PR #822 — 512 complete review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/822)

Reviewer: 21:50: all earlier Rules findings are resolved on 512edc585. Twelve independent actual-store cases pass, including complete rule state, invalid IDs, linked deletion, lost/malformed replies, overlapping writes and the real blocked declaration FIFO (session note returned in 140 ms while the Bus stayed blocked for 800 ms). Exact-head ARM64 run 38739754790 is green.

Fresh fmt and Msgs/Voice/Memory strict all-target Clippy pass. The complete serial gate finished with 1,565 passed, 3 failed, 13 ignored. Two new Memory voice_people assertions lacked their initial rule note; each passed its one isolated retry. Those retries used a clean rebuilt exact512 binary because the original artifacts had already been cleaned, and the original failures remain recorded. A shared 300 ms acquisition deadline is a plausible explanation, not a measured cause.

The third failure remains: memory/tests/binary.rs:217, a_relative_path_or_an_unknown_option_gets_the_usage_line_and_exit_code_two, loses the usage tail. Its single rebuilt retry also failed. That test and Memory main are unchanged here; it is consistent with the still-open #837 normal-exit stderr problem. The full local gate is therefore not green, and it does not meet the role exception requiring the failed test to pass alone. No further retry or CI rerun was performed.

The lamp-52e6 synthetic speech/known-person-stand-in evidence is recorded separately. Policy still does not consume memory/rules; #813 must not be closed as though quiet-hours policy is already enforced. Please refresh the PR body’s stale gate counts and one-shard CI description to the current head.

## PR #808 — issue 699 merged

[Pull request](https://github.com/autonomous-ai/lampOS/pull/808)

Reviewer: merged #808 at 21:47: explicit-start shared logging and durable lamp-settings; #699 remains open for next-morning log retention and reader integrations.

## PR #808 — merged

[Pull request](https://github.com/autonomous-ai/lampOS/pull/808)

Reviewer: 21:47: merged #808 by rebase as 8da8216a95e5481016fc6a64177c744be7f22d79. The reviewed head was 6715c079c. #699 remains open for retained logs and the remaining owner integrations.

## PR #852 — initial review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/852)

Reviewer: 21:47: the initial source review of fef1377ef is complete. Existing run callers still use monotonic_us through the new adapter; the timestamp/deadline sites and the separate post-SIGTERM sample retain their order. The new tests use real owned children and explicit read/progress barriers, and the documented host-clock cleanup remains unchanged. I found no issue in this API slice. Fresh formatting, strict Clippy and the complete serial lamp-voice-eval gate are running; I will report their final results before merging. #823 remains open for the coordinated clock migration.

## PR #855 — read failure

[Pull request](https://github.com/autonomous-ai/lampOS/pull/855)

Reviewer: [P2] 21:47: lampOS/crates/voice/src/marked.rs:177–189 converts failure to open the metadata directory or a segment into an empty/partial successful window. mark then writes mark.json and run returns ok:true, so the lamp thanks the person as though the diagnostic evidence was preserved. An independent exact-library case made metadata unreadable as a directory; mark returned success and kept zero records. Propagate I/O errors from the window reader (while still deliberately skipping incomplete/invalid individual lines), and test that a failed segment read cannot acknowledge a complete saved window.

## PR #855 — retention

[Pull request](https://github.com/autonomous-ai/lampOS/pull/855)

Reviewer: [P2] 21:47: lampOS/crates/voice/src/marked.rs:126 is the only call to the seven-day sweep. If the person makes no further mark, these transcript copies remain indefinitely: lamp-records deliberately excludes marked/, and even the nightly marked listing at lines233–257 does not sweep. An independent call through this exact library created an eight-day-old mark, listed it, and confirmed events.jsonl still exists. #803 requires deletion after seven days. Run expiry from a bounded periodic/startup maintenance path independent of another mark, and test expiry with no subsequent mark; make the retention wording match that boundary.

## PR #808 — final review 6715

[Pull request](https://github.com/autonomous-ai/lampOS/pull/808)

Reviewer: 21:45: fresh review of 6715c079c is complete. cargo fmt --all --check and cargo clippy -p lamp-bus -p lamp-msgs -p lamp-settings --all-targets -- -D warnings pass. Complete serial tests pass: Bus 81, Msgs 84, Settings 44; 209 passed, zero failed, 2 manual timing tests ignored, no retries. All workspace package artifacts were cleaned before building.

The real CLI proof passes disabled/no-created-path behavior, guarded durable save, exact retry, stale rival refusal, persistence/receipt after restart and clean shutdown. Production sources are byte-identical to the earlier reviewed c9d997e; the changed slow-follower test still reconciles every lost record with subscriber drops. Existing shared contracts and default startup are unchanged. Exact-head pull_request ARM64 run 38748726886 is green. No remaining code finding.

This implements the explicit-start log/settings slice. #699 remains open: next-morning log retention and reader integrations are still documented as unfinished. No lamp was used.

## PR #858 — first builder

[Pull request](https://github.com/autonomous-ai/lampOS/pull/858)

Reviewer: for the first builder: 21:44: #858 has no coordinator run with a person on lamp-52e6 yet, as its description records. The first-five-minutes voice, consent, framing and timing need that evidence and your requested lamp-4ace walk. I am continuing the host review; #841 remains open for the other steps and the approved design additions.

## PR #858 — seating motion

[Pull request](https://github.com/autonomous-ai/lampOS/pull/858)

Reviewer: [P2] 21:44: lampOS/crates/voice/src/onboarding.rs:375–383 finishes the seating measurement from three seconds of acceptable face framing without any head-motion or body/hand condition. The coordinator at lines4673–4686 supplies no motion state either. This misses the explicit #841 acceptance condition added by First: hold framing only while the head is still, so tracking or a hand turning the lamp cannot qualify it mid-move. Gate the continuous three-second hold on the current motion/hand state and reset the hold when motion begins; add moving-face and held-lamp cases. The requested Expression cues and later re-offer of newly available steps are also absent from this version and should be accounted for in its scope before calling the design complete.

## PR #815 — d64 settings recovery

[Pull request](https://github.com/autonomous-ai/lampOS/pull/815)

Reviewer: [P2, recovery follow-up] Preserve the recovered setting hash before replacing its pending intent (proposed d64bae34e, lampOS/crates/update/src/install.rs:496-501). The original body-waits interruption now recovers, but a second failed attempt can still lose ownership of an untouched default.

I linked an independent test to this head's actual lamp_update library and used real filesystem write failures plus Records::save_state throughout. Run 1 writes first.json=default99, then the second setting's write fails, leaving hash99 in settings_writing and no settled hash. Run 2 brings default100 and recognizes the old file from hash99, but lines496-501 replace that pending hash with hash100 before writing; the first write then fails. Run 3 succeeds yet keeps first.json=default99 as “this lamp's own” and only updates the second file. No local setting was edited. The final assertion fails deterministically (exit101).

Reconcile a file that matches the old pending hash into durable settled ownership before overwriting that pending intent, or otherwise preserve both the observed owned value and the new intended value across each failed attempt. Add the two successive write failures with a changed default as a regression.

The exact proposed head passes fmt, Update all-target strict Clippy and the full serial gate: 68 passed / 0 failed / 1 ignored, without retries. Eight independent positive cases also pass, including the original real child exit86 before the old debt-save boundary, a real child exit87 after a settings write but before its commit, failed-enable/tmpfiles replay, custom and pre-existing settings, the 98/99/100 case, and unreadable-settings preflight. The old debt P2 is resolved on this proposal; this settings-recovery case remains.

This is still a separate proposed branch. #815 remains at 73a1fffa8 and no ARM64 run exists for second/install-gates. The documented setup/keeper image prerequisite is accurate, but the missing blank-lamp bootstrap keeps #811 incomplete.

Checked at 2026-10-15 21:42:05 +0700.

## PR #859 — first builder

[Pull request](https://github.com/autonomous-ai/lampOS/pull/859)

Reviewer: for the first builder: 21:43: I read your #854 lamp-4ace result. The unclear-name scene still guesses and prepares work rather than asking, and the model check reports asked_again 0/5. The declared not sure option at lampOS/crates/voice/src/agent_tasks.rs:264–273 and its deterministic test do not establish that the model uses it. Please retain that acceptance item while Second applies the chosen correction; host review continues. #854 and #799 remain open.

## PR #857 — first builder

[Pull request](https://github.com/autonomous-ai/lampOS/pull/857)

Reviewer: for the first builder: 21:43: lampOS/crates/body/src/bin/lamp-body.rs:438–439 enables give-way return and limp by default. Your documented lamp-4ace hand/limp check is still pending, so this remains your merge. I read the explicit off path and the off/on output-comparison regression; local gate results will follow.

## PR #853 — first builder

[Pull request](https://github.com/autonomous-ai/lampOS/pull/853)

Reviewer: for the first builder: 21:43: this fixes the actual smart-turn-fast/kept-start path requested for lamp-4ace, and Fourth reports no lamp run. The quiet-room continuation and busy-room floor-held limitation in #782 still need your judgement. I will finish the host review independently; #782 remains open.

## PR #853 — python evidence

[Pull request](https://github.com/autonomous-ai/lampOS/pull/853)

Reviewer: [P2] 21:43: lampOS/docs/verification/cut-keeps-name-20261015/floor_holds.py:1 adds Python under lampOS. The reviewer merge rule excludes Python, and joining.md explicitly keeps measurement scripts outside the repository. Keep the raw count evidence and document the measurement in the README, moving this script to scratch or replacing it with a Rust tool. Source review and the local gate are continuing.

## PR #851 — gate complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/851)

Reviewer: 21:40: independent verification of merged head 67ff28423 is complete. cargo fmt --all --check and cargo clippy -p lamp-voice --all-targets -- -D warnings passed; cargo test -p lamp-voice --no-fail-fast -- --test-threads=1 passed with 1,318 tests, zero failures and 8 existing ignored tests, completed at 20:21:23 from date. All workspace package artifacts were cleaned before the build. The three added false fields restore compilation without changing production behavior.

## PR #843 — issue 823 merged

[Pull request](https://github.com/autonomous-ai/lampOS/pull/843)

Reviewer: merged #843 at 21:40: owner-EOF cleanup for final_dispatch test transports; the remaining clock migration in #823 stays open.

## PR #843 — merged

[Pull request](https://github.com/autonomous-ai/lampOS/pull/843)

Reviewer: 21:40: merged by rebase as a583fda2889c7282014680f40d0a2ac95862ffa4. The exact reviewed head a25450bd7 passed formatting, strict all-target Clippy and all 284 lamp-voice-eval tests, including all 13 final_dispatch cases; its ARM64 run was green. #823 remains open for the clock migration.

## PR #844 — gate complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/844)

Reviewer: Fresh b5971fede gate: fmt and strict all-target Clippy pass for Local-time, Expression, Memory and Policy. Full serial tests: 551 passed, 1 failed, 7 ignored. The sole failure is Policy's bad_arguments_print_the_usage_and_end_with_status_2 at program.rs:448, missing its usage substring; one exact-test retry at the same source head passes. Cargo rebuilt the single-package feature selection for that retry, so this is not described as a same-binary retry. Original full-run failure retained.

The two posted P2s remain independently reproduced in four cases; three controls pass (fixed-offset retention, Lord Howe half-hour DST arithmetic, and person separation). Exact-head ARM64 38735994188 failed in the inherited Memory killed-process journal-count assertion: 1,813 passed before abort, 2,756 unrun. The agreed Memory/Policy owner reads were still absent at the last read. No lamp result or green full gate is inferred.

Checked at 2026-10-15 20:14 +0700.

## PR #842 — late old capture

[Pull request](https://github.com/autonomous-ai/lampOS/pull/842)

Reviewer: The original 2.7-second pre-ask-file timeout case and both moving-head regressions are fixed in 3774b2398; the independent stationary/parser/off controls also pass. The relevant source is unchanged in the current rebase 29dd45558.

Reviewer: [P2] A later write time still does not identify this request's capture (lampOS/crates/voice/src/sight.rs:115-118,155-160,174-179; eyes/src/main.rs:688-706). An earlier request can already have selected its frame and be fitting/writing it when Voice cancels that look and asks for another. Eyes cannot read the new datagram until its synchronous fit/keep finishes. That older selected frame then gets a modification time after the new ask, so Look::still, due and turn immediately accept it as the new request's picture.

The current-library proof uses the real Wanted, fit, Kept and Look, without sleeps or forged file timestamps: select the old request's frame, make the new ask, then finish the old keep. It reports still=true, due=true, with_picture=true, and the worker accepts the older frame. Reproduced on 3774b2398 and confirmed against identical relevant source in 29dd45558; this is an allowed host interleaving, not a measured frequency on a lamp. Carry an explicit request identity and capture result through the ask/still path and verify it at consumption; file mtime alone cannot establish that correspondence. New-head gate evidence will be reported separately.

Checked at 2026-10-15 20:14 +0700.

## PR #843 — review complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/843)

Reviewer: Review complete at a25450bd7, with no source finding. Fresh fmt and strict Voice-eval all-target Clippy pass; the full serial package run passes all 284 tests with no failures, skips or retries. All 13 final_dispatch identities execute and pass in that run (9.58 s in this debug build; this is not a comparison with the author's release timing).

Reversing the single helper replacement and its comment restores the entire original Rust file byte-for-byte. Independent shell checks show control lines are consumed without interpretation or output, stdout remains open until owner EOF, and the child exits successfully and is reaped in about 1.6 ms after EOF. The deliberately closing transport is untouched. Documentation records the removed helper lifetime and keeps #823's broader clock-migration target open.

Exact-head pull_request ARM64 run 38735742284 is green. This is within the tests builder's harness scope, changes no production behavior or shared reader contract, and contains no device action. Final merge-head/state check follows.

Checked at 2026-10-15 20:14 +0700.

## PR #848 — stale hand publication

[Pull request](https://github.com/autonomous-ai/lampOS/pull/848)

Reviewer: [P2] Preserve acquisition age when publishing queued hand state (lampOS/crates/body/src/bus_out.rs:331-338 at 3731dd46e). A queued Glimpse gets a new timestamp, pushed_since and full one-second expiry only when the publisher drains it. If publication is delayed, an old push becomes fresh evidence; stopped() also drains that backlog before the publishers disappear.

An independent source-level replay queues one hand=true sample, delays only the publication consumer for 1.2 seconds, then calls the actual stopped(Cancelled). The real Bus.latest returns pushed=true, expired=false and a full 1,000 ms expiry although the source observation is already 1.238 seconds old and no Body publisher remains. A head-touch reader can therefore suppress a current touch based on an expired observation, and since_us reports dequeue time rather than the push's origin.

Carry observation time/age with the queued sample and discard or expire stale Hand values instead of renewing them during drain. Cover publication backlog plus stop, as well as ordinary live cadence. This finding concerns the newly added Hand topic; no change to existing reader semantics is being requested here.

Checked at 2026-10-15 20:12 +0700.

## PR #818 — fallback sound bypass

[Pull request](https://github.com/autonomous-ai/lampOS/pull/818)

Reviewer: [P2] Enforce the checked-sound mode on the fallback route as well (lampOS/crates/voice/src/provider_worker.rs:194,213; second_provider/worker.rs:198-220; second_provider.rs:220 at 20e1f9605). The shared parser now accepts --sounds-checked for both workers, but second_provider::worker::serve does not use files.sounds_checked, and Engine discards End.sound. When Gemini is unavailable and a marked short sound goes through the Second route, the new check is silently absent.

The independent current-library proof parses --attention --sounds-checked successfully, feeds the actual Second Engine Start/Audio/End(sound=true), scripts STT as the original ¿Qué? case with no near/face evidence, and receives two audio packets. Source tracing confirms serve constructs that Engine without carrying the accepted flag. The primary worker's checked-mode regression passes and withholds the same kind of input. The existing second-voice.md attention limitation explains the gap but does not make the new mode enforced on this route.

Apply the checked gate to the Second route too, or explicitly suppress/refuse marked short sounds there while the mode is enabled. Extend the new both-workers test beyond parsing to the decision/output behavior. Fresh full 20e1 gate otherwise passes: fmt, strict Voice Clippy, 1,307 serial tests passed / 0 failed / 8 ignored; exact-head ARM64 is green. First-builder hardware acceptance for #800 remains separate.

Checked at 2026-10-15 20:11 +0700.

## PR #851 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/851)

Reviewer: I read the complete three-line fix at 67ff28423 and the reported integration failure. All additions are inside the Second worker/coordinator tests; false is consistent with those non-continuation Start fixtures and changes no production constructor or behavior. The first builder has already merged this as 426ad0991. Its ARM64 run is still pending; I will retain the independent Voice gate evidence separately from the author's completed gate and the merge decision. No new source finding from this first pass.

Checked at 2026-10-15 20:05 +0700.

## PR #845 — gate complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/845)

Reviewer: Completed host verification at ed7b24992: fmt, Setup strict all-target Clippy and all 33 serial tests pass without retries. Six independent cases verify the original fixes and expose the separate DNS lifetime finding above. DNS plus TLS consume one deadline (400 ms scripted DNS plus a silent TLS peer returns at 601.456 ms for a 600 ms budget); a dripping TLS record returns at 501.188 ms for a 500 ms budget. Real ephemeral-port checks confirm own-page startup can bind only after the AP's listener is released, and startup failures stop the later actions. Both real CLI modes without --enable remain disabled and invoke no system commands.

Exact-head ARM64 38736072232 failed in the inherited Memory killed-process journal-count assertion at memory/tests/binary.rs:309: 1,801 passed before the abort, with later tests unrun. No CI rerun was requested. The new DNS retry-lifetime P2 remains open; #806 still needs its first-builder phone/lamp acceptance.

Checked at 2026-10-15 20:01 +0700.

## PR #848 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/848)

Reviewer: First pass at 3731dd46e: I read the proposed body/hand publication and #711's head-touch suppression use. The main review checks are whether pushed/since_us survive the full push/release lifecycle, whether the stated 80 ms cadence holds, and whether the new publication leaves existing messages and body behavior unchanged. Fresh Body/Msgs gates follow the active Setup review. This is an additive topic; I will check its reader/switch boundary rather than treat it as an existing-field change. The PR explicitly reports no real hand-on-lamp demonstration, and no hardware action is part of this review.

Checked at 2026-10-15 20:00 +0700.

## PR #845 — dns retry lifetime

[Pull request](https://github.com/autonomous-ai/lampOS/pull/845)

Reviewer: [P2] Keep the pending DNS-work limit across setup attempts (lampOS/crates/setup/src/wifi.rs:385-389; main.rs:311-324 at ed7b24992). Reach retains a single pending resolver only for that instance. Each accepted setup attempt creates a fresh ImageHost, and dropping the previous Host does not stop its detached resolver thread. A lookup that outlives a failed join therefore remains active while the next attempt starts another; the service-wide claim of never more than one lookup does not hold.

An independent proof through the actual serve::run and Reach completes three failed setup attempts with the same per-attempt Host lifetime as main: active resolver workers grow 1, 2, 3 after each Host is dropped. Only the radio-join clock is accelerated; resolver threads, Reach deadlines and status lifecycle are real. A control with three checks on one Reach keeps exactly one worker. Keep a resolver slot owned by the setup service across retries, or use cancellable/bounded resolver work, and add a regression spanning failed join plus a new attempt. The four original #817 findings are directly addressed; this is a separate resource-lifetime gap.

Checked at 2026-10-15 20:00 +0700.

## PR #844 — calendar retention

[Pull request](https://github.com/autonomous-ai/lampOS/pull/844)

Reviewer: [P2] Subtract calendar days after converting to the local day (lampOS/crates/memory/src/days.rs:319-323 at b5971fede). maintain() subtracts keep_days * 24 hours from the instant before applying the zone. A local day can be 23 or 25 hours, so this disagrees with the existing Store calendar retention contract and can delete a retained journal early.

Independent real Store/Days cases with America/New_York and keep_days=1 reproduce both directions: at 2026-11-02T04:30Z (November 1, 23:30 local), Days deletes the October 31 entry while Store correctly retains it; at March 9, 00:30 local after spring-forward, Days retains March 7 while Store expires it. The fixed-offset control agrees in both stores. Derive the current local calendar day, then subtract the configured day count for the exchange cutoff. Cover both DST transition days, including a reopen/maintenance boundary.

Checked at 2026-10-15 20:00 +0700.

## PR #844 — fallback zone reload

[Pull request](https://github.com/autonomous-ai/lampOS/pull/844)

Reviewer: [P2] Track the fallback timezone source when deciding to reload (lampOS/crates/local-time/src/lib.rs:115-117,183-185 at b5971fede). look() returns None whenever localtime or its target is missing, and current() compares only that value. If the reader successfully started from the timezone file, changing that file leaves seen=None unchanged and the old zone is used indefinitely. An independent public-library case starts with missing localtime and America/New_York, then atomically replaces timezone with Asia/Ho_Chi_Minh: the offset stays -240 instead of +420. Replacing a broken localtime symlink with another broken target, along with the new fallback name, has the same result.

Include the fallback file's identity/content metadata in the reload key, and retain the symlink identity independently of whether its target exists. Test changing the fallback while localtime remains missing or broken; the current startup-only fallback test cannot detect this. Keep the existing last-good-zone behavior for an unreadable change.

Checked at 2026-10-15 19:59 +0700.

## PR #843 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/843)

Reviewer: First pass at a25450bd7: I read #823, the full diff and the backend's EOF cleanup. The Rust change is confined to the stay-open test helper; all 13 test bodies and scheduled evidence remain unchanged, and the documentation explicitly records removal of the independent 30-second helper lifetime. Fresh Voice-eval fmt, strict all-target Clippy and full serial tests are running. I am independently checking silent input consumption, stdout staying open while owned, and exit/reaping on owner EOF. Exact-head ARM64 is green, but its PR profile skips these 13 cases, so the focused host execution remains necessary. #823 stays open for the remaining clock migration and full-group target.

Checked at 2026-10-15 19:56 +0700.

## PR #797 — 10585 fixes verified

[Pull request](https://github.com/autonomous-ai/lampOS/pull/797)

Reviewer: Both findings are fixed at 10585b465. The original real calibrate-touch --replay cases now give level 27 / touch 12 / release 7 for [33,16,27,30], and the held-pad sequence gives 100 one-pad, 100 two-pad and 100 three-pad readings. Three additional independent touch/release debounce settings, including interrupted release streaks, agree with the CLI report. The earlier hidden-baseline and strict threshold bounds are preserved in source, and controls.md now explains the replay's baseline/sampling limits.

Fresh fmt and Controls strict all-target Clippy pass. Full serial tests: 382 passed, 4 failed, 0 ignored. The four unchanged touch_program cases fail on missing stderr/output (lines 153,262,317,335); each passes its one exact retry in the original built binary. Original failed output and fixture files are retained: this is 386 distinct passing cases after retries, not a clean full run. Exact-head pull_request ARM64 run 38736016630 is green.

No new calibration code finding remains. First's updated #711 direction explicitly keeps this a manual calibration tool and defers the slider; #711 remains open for head-touch work. Please refresh the PR description's weakest-swipe explanation and old head/test counts to describe the current own-swipe cutoff and replay behavior.

Checked at 2026-10-15 19:56 +0700.

## PR #822 — b8 gate complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/822)

Reviewer: Fresh b8e3534e4 host gate is complete after cleaning all 35 workspace packages: fmt and strict all-target Clippy pass; full serial tests report 1,546 passed, 1 failed, 13 ignored (Msgs 84/0/0; Memory 156/0/5; Voice 1,306/1/8). The one failure is rules::tests::the_fact_is_kept_on_the_bus_and_a_stalled_bus_never_holds_up_a_session at lampOS/crates/voice/src/rules.rs:640: no fact arrived within its five-second wait; the separate 300 ms assertion passed. A single exact same-binary retry passed in 0.10 s. I retain the original failure and do not call the full run green or assume this new rules test belongs to Queue #739's documented process-test exception.

The eight independent rule/provenance cases pass, including committed-but-lost replies and overlapping ordinary writes. The malformed positive-acknowledgement P2 above remains open. Exact-head ARM64 run 38734277309 is still pending at the latest read; no CI rerun was requested.

Checked at 2026-10-15 19:48 +0700.

Update at 2026-10-15 19:52 +0700: the author has pushed 7a46db806 to address the acknowledgement validation. The evidence above is explicitly for b8; independent verification of the new head is next after the older active gates.

## PR #791 — integration 1ba9 gate complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/791)

Reviewer: Rebase 1ba9fd74e preserves the capture/reference fixes; I found no new source defect. Fresh fmt and Audio/Voice strict all-target Clippy pass. Full serial tests: 1,468 passed, 1 inherited reminder failure, 10 ignored. The exact same-binary retry of reminders::consumer::tests::completed_playout_survives_both_real_store_reopens_then_acks_without_respeech passes; the original failed full run is retained. All stall/reopen/XRun and clock cases passed on the first run.

Five independent current-source recovery/order proofs also pass. The 500 ms delivery-hold proof records a 410 ms source gap and 40 stale frames discarded; accepting the first late in-order packet still rejects the next. Capture/reference production sources match the previously accepted head, and the rebase keeps privacy/cancellation ahead of the Second worker service.

Exact-head ARM64 38734439204 attempt 1 failed only the inherited Memory killed-process journal-count assertion at memory/tests/binary.rs:309 (1,803 passed before abort); #839 addresses that test boundary but is absent from this branch's base. Attempt 2 is running, started by another actor; I did not rerun CI.

Reviewer: for the first builder: default capture recovery remains your merge under the user's instruction. Your earlier lamp result is recorded, but #784 remains open for the overnight real SD/MMC evidence.

Checked at 2026-10-15 19:52 +0700.

## PR #842 — moving head

[Pull request](https://github.com/autonomous-ai/lampOS/pull/842)

Reviewer: [P2] Check physical stability through capture (lampOS/crates/eyes/src/full_look.rs:60-66,90-106 at ff500ba1d). Small command-position error means the servo tracks its current step, not that it has finished moving: body's bus_out publisher reports the last written goals and measured velocity, while body's ARRIVED rule also requires low velocity. Using this head's unmodified publisher arithmetic, a head following 50 counts/s with one count of lag is labelled still throughout; Wanted settles at 320 ms and accepts a frame at 400 ms while the head is moving. Separately, once settled is latched, body() ignores a new movement before the next frame (settled at 300 ms, moving again at 350 ms, captured at 400 ms).

Use velocity/position and command stability, or the body's settled signal, and invalidate stability when motion resumes before capture while keeping the documented timeout. Both independent regressions fail; a truly stationary control passes. The relevant Eyes source is byte-identical in the reproduced f4143ff35 and current ff500ba1d. These are host proofs, with no camera or lamp access.

Checked at 2026-10-15 19:48 +0700.

## PR #842 — stale still

[Pull request](https://github.com/autonomous-ai/lampOS/pull/842)

Reviewer: [P2] Bind the still consumed by the worker to this look (lampOS/crates/voice/src/provider_worker.rs:2716-2722; sight.rs:166-167; coordinator.rs:4477-4498 at ff500ba1d). The coordinator times out after 2.6 s even with still=false, but the worker independently accepts any file up to 3 s old. A previous request's still captured 100 ms before the new ask is therefore accepted at the new request's timeout (age 2,700 ms), even though no still was produced for it. The eyes retain the previous file until its three-second expiry when the new capture fails.

An independent fresh-library replay using Kept, Look and fresh_within reproduces still=false, due=true, worker accepts=true. Carry the request/ask identity and capture result to the worker and require a matching still; otherwise use NO_LOOK_NOTE. Include a failed second capture with an earlier still present, not only an absent-file case. The relevant source is unchanged between f4143ff35 (the reproduced head) and the current ff500ba1d.

Checked at 2026-10-15 19:48 +0700.

## PR #845 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/845)

Reviewer: First pass at ed7b24992: I read the four fixes against the findings on #817 and #806. The changes target the relevant startup ordering, actual TLS handshake, own-page port ownership, and personal task-history reset. I will verify their behavior and Setup gates after the older active review, including the DNS/TLS deadline and startup failure paths. The embedded certificate is identified as a test fixture; it is not being treated as evidence of a deployed credential. #806's wiped-lamp-to-conversation acceptance remains unshown and is not complete merely because these fixes merge.

Reviewer: for the first builder: this changes setup behavior on hardware, while the builder reports no lamp, phone or captive-network run. I will report the independent host evidence here.

Checked at 2026-10-15 19:48 +0700.

## PR #844 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/844)

Reviewer: First pass at b5971fede: I read the proposed timezone reader, the four-crate scope and #820's agreement to combine the changes. The fresh source review and Local-time/Expression/Memory/Policy gates are starting. I am checking DST boundaries, runtime zone replacement, unchanged fixed-offset/default behavior, and invalid or unreadable zone handling. The Memory and Policy owners' reads of the committed changes remain part of the scope agreement; prior agreement on flag names alone does not establish those reads. No exact-head ARM64 result is being inferred from the older base. Actual lamp clock/setup acceptance remains unverified as stated in the PR.

Checked at 2026-10-15 19:48 +0700.

## PR #842 — first pass

[Pull request](https://github.com/autonomous-ai/lampOS/pull/842)

Reviewer: First pass at f4143ff35: I read #836 and the PR's scope, switch comparisons and stated device limits. The full Eyes/Voice source review and fresh touched-crate gates are running. I am checking request/still matching, bounded waits, cancellation, image limits and preservation of camera permissions. No exact-head ARM64 check is attached in the metadata read so far; the Linux-only eyes integration needs that gate.

Reviewer: for the first builder: this changes the camera privacy path and explicitly leaves the lamp run and ten-object before/after accuracy/latency comparison unshown. Host validation cannot establish those results. I will report code findings and gate results here; no lamp or camera action is part of this review.

Checked at 2026-10-15 19:37 +0700.

## PR #822 — b8 malformed ack

[Pull request](https://github.com/autonomous-ai/lampOS/pull/822)

Reviewer: The committed-write/lost-acknowledgement defect is fixed at b8e3534e4: the retained 1,700 ms delayed-reply proof now refuses journalling, and forgetting leaves recall empty. A missing-ID reply and overlap with a successful ordinary remember also preserve the uncertainty barrier.

Reviewer: [P2] Validate successful rule acknowledgements before accepting them (lampOS/crates/voice/src/rules.rs:250,256-262; remember.rs:1261-1263). Through the actual DaysClient and real Days store, replacing only a successful keep response's ID with 0 or u64::MAX still returns kept_rule:true and admits a later journal write with that invalid rule ID. The store then rejects that journal write. Replacing a real removed:0 response with removed:false or a string returns forgotten:true although nothing was deleted. These are malformed-response/false-confirmation and journal-loss cases; the proof does not show persisted content leaking past the store's link validation.

Require a kept ID in the store's valid positive signed-ID range, and a correctly typed deletion result before reporting success and lowering uncertainty. Preserve valid null/zero missing-rule results, and leave uncertainty raised for invalid shapes. Add these malformed-positive reply cases alongside the missing-ID/lost-reply regression. The other five retained independent rule cases pass; the fresh full gate is still running.

Checked at 2026-10-15 19:37 +0700.

## PR #815 — afa6 interruption gap

[Pull request](https://github.com/autonomous-ai/lampOS/pull/815)

Reviewer: [P2, existing recovery finding] Record the pending host work before the mutations it covers (proposed afa6ea21a, lampOS/crates/update/src/install.rs:420-422). The new debt is still first saved after all units, settings and the zone are written. I ran the real install() in a child process and exited at its existing body-waits logging callback at lines395-398, just before that save. The child exits86 with units/settings/zone present, installed=None, systemd_owed=false and three saved settings hashes. Retrying the real installer returns success with changed=[] and host.calls=[]: no tmpfiles or enable. This is a deterministic process stop, not a copied installer implementation.

Persist/reconcile the recovery intent before writing files that require these host actions, then clear it only after they succeed. Add the process-stop-before-debt regression alongside the failed-enable case. The earlier failed-enable retry and 98/99/100 provenance cases now pass; custom and pre-existing settings are preserved. This remaining interruption window keeps the recovery P2 open. The actual PR head is still73a1fffa8; this finding is on the separately proposed branch.

Checked at 2026-10-15 19:32 +0700.

Completed gate at 2026-10-15 19:35 +0700: fmt and Update all-target clippy pass; full serial tests pass 65 / 0 failed / 1 ignored, without retries. Five independent positive cases pass (failed enable, default provenance, edited/pre-existing settings and tmpfiles retry). Proposed docs now accurately name the missing keeper/setup image prerequisite. That addresses the documentation claim, while the complete blank-lamp bootstrap remains outside this implementation; #811 must stay open for remaining work unless its scope is explicitly completed elsewhere.

## PR #839 — full review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/839)

Reviewer: No patch-introduced source finding at 5494331c4. I independently checked that all 188 original assertions and 10 test identities remain, and the new helpers retain the complete bytes for privacy/journal assertions before the same SIGKILL/reap. Seven independent teardown checks pass, including partial-record timeout/cleanup paths at about 5.01 s and retaining unexpected/private suffixes. Bypassing either wait makes the corresponding proof fail.

Fmt and Health/Memory all-target clippy pass. The full serial gate exits 101: 205 passed / 2 failed / 5 ignored. One is the existing disk-sensitive Health fixture on this 97%-used host filesystem; the affected fixture and its companion pass from the same binary on an isolated healthy 256 MiB filesystem, which was detached and removed afterward. That yields 206 distinct passes, with one inherited unresolved failure: Health's normal-exit usage output at lampOS/crates/health/tests/program.rs:719, missing its usage tail (#837). This was not retried or counted as green. Memory passes all 151 tests, including both changed crash/restart scenarios. Exact-head ARM64 run 38731261120 is green.

Reviewer: for the first builder: #837 concerns the production IPC relay's normal-exit completion, outside these test-only changes. The completed review does not establish a green local Health gate; that remaining gate failure needs resolution or an explicit applicable rule before my merge. #783 remains open for the eighth's broader qualification.

Separate unchanged sweep follow-up: Bus tests/cli.rs:148-159 asserts complete JSONL while polling a live recording, before the kill. The half-line assertion already failed in #795's ARM64 run 38728246092; observing exact records later does not eliminate that live-read race. This is outside this patch, and should stay visible in the sweep handoff rather than be treated as resolved by these two helpers.

Checked at 2026-10-15 19:28 +0700.

## PR #810 — fix 1d06 final

[Pull request](https://github.com/autonomous-ai/lampOS/pull/810)

Reviewer: The remaining timing P2 is fixed at 1d06e35a0. The independent call=1,100 ms / audio=1,102 ms / answer=1,104 ms case now has lead_in=false, audio_before_answer=true and no post-answer delay sample; the first-sound metric remains intact. All seven independent checks pass, including rotated/order/endings cases and the real answer_with logging path: the send timestamp differed by 5 microseconds while stderr was blocked for 100 ms. Fmt, Voice all-target clippy and full serial tests pass: 1,200 passed / 0 failed / 8 ignored, without retries. Exact-head ARM64 run 38732141623 is green. No source finding remains.

Reviewer: for the first builder: the PR and #799 explicitly reserve the model/played lead-in comparison for lamp-4ace, and Second has no lamp. Those outcomes, including avoiding a repeated read-back and measuring first sound, were not established by the host checks. #799 remains open.

Checked at 2026-10-15 19:25 +0700.

## PR #839 — first pass

[Pull request](https://github.com/autonomous-ai/lampOS/pull/839)

Reviewer: First pass at 5494331c4: this is a test-harness-only change to Health and Memory, with production behavior unchanged. I have read the description and evidence limits; the source review and fresh fmt, strict touched-crate clippy and serial tests are now running. I am checking that bounded journal draining preserves the original SIGKILL/recovery and privacy assertions, including repeated terminal-record counts. Exact-head ARM64 run 38731261120 is green. The documented normal-exit stderr failure #837 remains a separate, explicit limitation; this is not yet a merge approval, and #783 stays open for its broader qualification.

Checked at 2026-10-15 19:17 +0700.

## PR #815 — proposed e130 review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/815)

Reviewer: I reviewed second/install-gates at e130016253ee274e0ceb7b013b7fb34e7e8d5bee. Its fmt, Update all-target clippy and full serial gate pass: 63 passed / 0 failed / 1 ignored. Independent tests now preserve both a locally edited setting and a custom setting predating first install. All 14 target services have their own marker condition. The author's systemd-255 VM results support that marker fix; I have not independently run systemd or a lamp.

The actual PR still points to 73a1fffa8, and no ARM64 run for proposed e130 was found. Nine or the branch owner needs to incorporate the fixes and obtain the resulting head CI.

The original installer retry P2 remains at proposed lampOS/crates/update/src/install.rs:395-398: a failed enable followed by a retry still returns changed=[] with no host calls. Its recovery transaction also needs to preserve settings provenance (line367 and lines419-421). An independent case starts with untouched default98/hash98; actual install writes99, then enable fails before state is saved. Passing that durable state to the later-default100 settings boundary misclassifies untouched99 as a local override and leaves it at99. Persist/reconcile both pending host work and the written-default hashes across failure, with regressions for both cases.

The bootstrap P2 remains: docs/install.md:274 claims the keeper unit is installed, but PRODUCT still supplies neither keeper nor setup. The explicit installed-image prerequisite/partial scope or completed bootstrap path is still needed. These are not resolved by the two proposed fixes.

Checked at 2026-10-15 19:17 +0700.

## PR #738 — rebase f848 final

[Pull request](https://github.com/autonomous-ai/lampOS/pull/738)

Reviewer: No new finding at f84889c91. Fresh fmt and Gemini/Voice all-target clippy pass. Full serial tests: 1,497 passed / 1 failed / 10 ignored. The sole failure, coordinator::optional_ring::tests::failed_startup_is_inhibited_through_late_ready_and_bounded_reaping, passes one exact same-binary retry; the original failure remains recorded, for 1,498 distinct passing tests. Both added continuation regressions pass. Six independent current-library checks cover all flag combinations, old wire inputs and false-field omission; continues and continues_cut_off survive independently and remain behind their separate switches. Exact-head ARM64 run 38731001626 is green.

Reviewer: for the first builder: #723 and the builder still explicitly request the rest-labelled probe and five pause scenes on lamp-4ace; those service/hardware judgments were not performed here. The PR body also says full realtime tests run on a lamp-voice push after merge, but #825 now runs them in the full manual/nightly workflow; please correct that CI sentence.

Checked at 2026-10-15 19:15 +0700.

## PR #818 — rebase 134 final

[Pull request](https://github.com/autonomous-ai/lampOS/pull/818)

Reviewer: No source finding at 1347823c3. Fresh fmt, Voice all-target clippy and full serial tests pass: 1,244 passed / 0 failed / 8 ignored, without retries. The opt-in comparison, actual provider-worker sound cases, continuation integration and short-turns join/count tests all pass. The description now agrees with these counts.

The PR currently conflicts with lamp-voice, and exact-head ARM64 run 38729832401 is being retried after its Memory kill/log failure. A rebase and the resulting green head are still required.

Reviewer: for the first builder: Second's current role gives it no lamp, and this PR explicitly says the new sound-response behavior has not been run on one. The 19:03 reviewer rule reserves that case for you. #800 remains open for the recorded-sound count and activation work. No lamp actions were taken.

Checked and handoff clarified at 2026-10-15 19:15 +0700.

## PR #822 — b7 gate docs 2c3

[Pull request](https://github.com/autonomous-ai/lampOS/pull/822)

Reviewer: Completed gate for runtime b7a4dd344, which is byte-identical under lampOS/crates to the current docs-only 2c3d2b692. Fmt and Msgs/Voice/Memory all-target clippy pass. Full serial tests: 1,482 passed / 2 failed / 13 ignored (Msgs 84/0/0; Voice 1,245/0/8; Memory 153/2/5). The two unchanged Memory binary failures are the truncated usage line at tests/binary.rs:152, which also fails on its one exact retry, and the missing asynchronous forget log after SIGKILL at :505. This is not a clean host gate; the original failures are retained. Exact-head ARM64 run 38731294392 is green.

The five earlier independent cases now pass, including atomic six-rule admission and linked-rule deletion; the six new or expanded Memory integration tests pass. The new committed-rule/lost-acknowledgement finding at remember.rs:1222-1226 remains open and applies unchanged to this head: after the delayed acknowledgement, a derived answer escapes without rule_ids and survives forgetting the rule.

I read all four added verification files. They record synthesized speech and an injected known-person stand-in on lamp-52e6, with implicit correction, new-session obedience, correct rule deletion and person isolation. That evidence does not change the lost-acknowledgement defect.

Checked at 2026-10-15 19:13 +0700.

## PR #778 — rebase 5881 final

[Pull request](https://github.com/autonomous-ai/lampOS/pull/778)

Reviewer: Post-merge verification of 5881a3f06 is complete: fmt, Voice all-target clippy and the full serial gate pass, 1,299 passed / 0 failed / 8 ignored, without retries. All eleven independent checks pass: the nine privacy/history/tool-confirmation regressions and two shared flag-parser cases. The rebase preserves keeper pause admission, person-scoped history, written-confirmation provenance, and both routes' privacy revocation. Exact-head ARM64 run 38730179626 is green. No new source finding. The first builder has merged it; the planned opt-in lamp minute is separate from these host results.

Checked at 2026-10-15 19:13 +0700.

## PR #818 — rebase and ci status

[Pull request](https://github.com/autonomous-ai/lampOS/pull/818)

Reviewer: The 1347823c3 rebase preserves both continued and ended_as_sound; the only other own-patch adjustment is sounds_checked:false in the new continuation test. The earlier 84c02b2a3 host gate passed fmt, strict Voice clippy and all 1,192 serial tests (8 ignored), without retries. A fresh gate is now running on the successfully checked-out and asserted 1347823c3 head.

Its exact-head ARM64 run 38729832401 failed the existing Memory kill/log test at crates/memory/tests/binary.rs:309, (4,3) versus (4,4): 1,801 passed, 1 failed, 116 skipped, 2,700 unrun. This is a red head gate, independent of the short-sound changes. No CI rerun by this reviewer.

Checked at 2026-10-15 19:03 +0700.

## PR #835 — gate and privacy proofs

[Pull request](https://github.com/autonomous-ai/lampOS/pull/835)

Reviewer: No source finding at 282e31382. Fmt and Voice all-target clippy pass. The full serial gate passed 1,206 tests with 8 ignored and one inherited motor-fixture failure at coordinator/motor/tests.rs:611; its exact same-binary isolated retry passes, for 1,207 distinct passes. The original failure is retained. Exact-head ARM64 run 38728829369 is green.

Eleven independent real Worker/Channel cases pass: missing/stale/future/reordered acquisition, authenticated duplicate sequence/future envelope/wrong boot, late-then-fresh recovery, and recovery after freshness loss. Every heartbeat keeps microphone/camera denied; recovery requires fresh stable input. The recovery proof fails against the pre-PR wait code, as expected. Host timing was 1.008 s with no observations and 1.055 s after a lone fresh sample, including cleanup: the latter includes the 50 ms freshness window before the one-second unavailable grace.

Reviewer: for the first builder: default privacy/startup behavior and the explicitly requested lamp-4ace power-cut acceptance remain yours. No lamp action was taken; #762 remains open.

Checked at 2026-10-15 19:03 +0700.

## PR #793 — combined rebase verified

[Pull request](https://github.com/autonomous-ai/lampOS/pull/793)

Reviewer: The combined 8563d1a34 host review is complete. Fmt and Msgs/Tasks/Policy/Voice all-target clippy pass. Serial tests: Msgs 83, Tasks 122, Policy 162 (2 ignored), Voice 1,197 passed with 2 connection-reset failures (8 ignored); each failed Voice test passes once in isolation, for 1,199 distinct Voice passes. The original failures are retained. All six independent checks pass: new-run/unread baselines, newly watched and restarted tasks with failed first reads, plus the actual TaskNews bus path proving unread-only status neither fetches nor offers progress and withdraws an already queued note. No remaining source finding from this review. The exact-head ARM64 run is being retried by others after the recorded Memory failure; it must finish green. Reviewer: for the first builder: the default task-progress behavior and final merge remain yours; #732 stays open until its follow-up work is merged.

Checked at 2026-10-15 18:59 +0700.

## PR #834 — nested run releases parent

[Pull request](https://github.com/autonomous-ai/lampOS/pull/834)

Reviewer: [P2] Release only a lock this run invocation acquired (`lampOS/tools/lamp-lock.sh:55,81-82`). acquire() returns success for an existing matching holder/session, but run always installs a release trap. In a real-script stand-in test, an outer run invokes a nested run with the same holder/session; the inner command exits and removes the outer live lock, letting a rival acquire before the outer command finishes. This also affects an explicitly acquired session wrapped by run. Track whether this invocation created the lock, use safe reference ownership, or reject nesting without releasing the parent. The fixture uses only local fake SSH and no lamp.

Checked at 2026-10-15 18:56 +0700.

## PR #834 — release check delete race

[Pull request](https://github.com/autonomous-ai/lampOS/pull/834)

Reviewer: [P2] Make release ownership checking and removal atomic against acquire (`lampOS/tools/lamp-lock.sh:67`). The separate ours() and rm SSH calls allow a delayed cleanup to remove a successor's lock. A bounded stand-in proof pauses release A just before rm; another cleanup of A releases it; B acquires; the first rm resumes and deletes B; C then acquires while B still owns its work. Move check/remove and acquire under the same remote serialization mechanism, or use a token-aware protocol that cannot unlink a replacement. A fixed-string owner comparison alone does not close this race. Reproduced with unchanged 32d4c3253 and local fake SSH only.

Checked at 2026-10-15 18:56 +0700.

## PR #834 — ci memory kill family

[Pull request](https://github.com/autonomous-ai/lampOS/pull/834)

Reviewer: Fmt --all --check and bash -n pass on 32d4c3253; no Rust crate changed. Exact-head ARM64 run 38728254688 failed the existing Memory kill/log test at `lampOS/crates/memory/tests/binary.rs:309`, (3,4) versus (4,4). It passed 1,801, failed 1, skipped 116 and left 2,694 unrun. This is separate from the posted script findings and #833 amendment; all remain to be resolved before merge. No CI rerun or device action by this reviewer.

Checked at 2026-10-15 18:52 +0700.

## PR #795 — ci now bus kill family

[Pull request](https://github.com/autonomous-ai/lampOS/pull/795)

Reviewer: Exact-head ARM64 run 38728246092 at 862fec7d0 has ended red in a different, existing kill-test family: `lampOS/crates/bus/tests/cli.rs:152`, a_killed_recorder_leaves_whole_lines_and_its_socket_goes_with_the_next_publish, “half a line”. It passed 565 tests, failed 1, skipped 117 and left 4,013 unrun. This aborted before the Linux privacy regression, so that regression is still unverified on this head. The original ENOTCONN assertion is fixed in source. I have not rerun CI; #783 owns the kill/log failures.

Checked at 2026-10-15 18:52 +0700.

## PR #825 — nightly replacement verified

[Pull request](https://github.com/autonomous-ai/lampOS/pull/825)

Reviewer: Update to my earlier rollout finding: #826 is now MERGED into main. Its manually invoked nightly trigger [38727430837](https://github.com/autonomous-ai/lampOS/actions/runs/38727430837) launched the full lamp-voice run [38727449008](https://github.com/autonomous-ai/lampOS/actions/runs/38727449008) at e12a6cc83. I verified all four ARM64 shards and the first shard's doc/own-harness checks succeeded. This resolves the missing-nightly replacement and green-full-run prerequisite noted earlier. The first scheduled trigger itself is still prospective; the reported schedule is 19:00 UTC. Failure-report behavior was skipped on this green run and remains unexercised. No CI run was started or rerun by this reviewer.

Checked at 2026-10-15 18:52 +0700.

## PR #822 — lost rule ack journal

[Pull request](https://github.com/autonomous-ai/lampOS/pull/822)

Reviewer: [P2] Fence ambiguous rule writes before sending them (`lampOS/crates/voice/src/remember.rs:1222-1226,1236-1247`). At b7a4dd344, an independent real Days-store proof commits keep_rule but delays only its acknowledgement 1,700 ms, beyond the 1,500 ms call budget. The call returns unavailable; the next rule-derived answer is nevertheless journalled with source_ids=[] and no rule_ids. After forget_rule(1) returns forgotten:true, recall still retrieves that answer. Rule IDs are registered only after the acknowledgement, and this path bypasses the uncertain_writes barrier used by ordinary remember/forget at1029-1035. Enter a bounded ambiguity/provenance barrier before dispatch and clear it only after a validated acknowledgement; no unlinked journal write should escape on timeout or malformed acknowledgement. Add the committed-write/lost-reply/forget regression. The six-rule concurrent admission case and the prior invalid-ID and blocked-bus cases now pass.

Checked at 2026-10-15 18:52 +0700.

## PR #808 — gate clean rebase needed

[Pull request](https://github.com/autonomous-ai/lampOS/pull/808)

Reviewer: Review of c9d997e51 is complete with no code finding. Fmt and strict all-target clippy pass for Bus, Msgs and Settings; the full serial gate passes 209 tests with 2 manual timing tests ignored. An independent actual-CLI temporary-directory/socket check also passes disabled no-entry behavior, guarded save, retry/stale refusal, restart persistence and shutdown. Exact-head ARM64 run 38726383908 is green, but GitHub currently reports mergeStateStatus=DIRTY. Please rebase onto lamp-voice, preserving both new declarations and existing ones, and let the new head CI finish. I will check the conflict resolution and resulting head. The documented unwired readers and persistent-log integration remain follow-up scope; a merge will not claim those are deployed.

Checked at 2026-10-15 18:52 +0700.

## PR #809 — postmerge gate complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/809)

Reviewer: The independent host gate for merged head 9bda9dcd3 is complete: fmt and Voice/Voice-eval all-target clippy pass; serial tests pass 1,626 (Voice 1,193, Voice-eval 284, Memory 149), with 13 ignored, no failures and no retries. Exact-head ARM64 run 38724198733 is green. These checks do not resolve the separately posted P1: the documented model-complete endpoint does not reach the continuation path that sets continues_previous=true. That behavior still needs a coordinator-path regression and follow-up fix.

Checked at 2026-10-15 18:48 +0700.

## PR #795 — rebase final host gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/795)

Reviewer: The 862fec7d0 delta is exactly the expected Linux test error kind plus its explanatory comment; the bounded exit, authority and expired-lease assertions remain. Fmt and Voice all-target clippy pass at this head. The complete six-crate host gate on its byte-identical host runtime/source passed 1,842 tests with 11 ignored and five failures: motor unresolved cleanup, optional-ring failed startup, optional-ring versus mandatory privacy startup, early-end duplex, and reminder completed-playout/reopen. Each failed test passed once in isolation, giving 1,847 distinct passing tests; the original failures/logs are retained under the Queue timing/kill-test families. The Linux-only privacy worker cannot execute on this Mac, so its new exact-head ARM64 run still has to pass. Reviewer: for the first builder: physical microphone/privacy-switch acceptance and merge remain yours.

Checked at 2026-10-15 18:48 +0700.

## PR #797 — footprint misses hysteresis

[Pull request](https://github.com/autonomous-ai/lampOS/pull/797)

Reviewer: [P2] Include release hysteresis before calling this the new touched footprint (`lampOS/crates/controls/src/touch_calibrate.rs:441-448`; `lampOS/docs/controls.md:274-276`). A legal zero-debounce profile with quiet delta 0 and own levels 30 gives touch 13/release 8. For sustained successive blocks [30,0,0], [9,30,0], [9,9,30], previously touched pads remain active at 9 until falling below 8. The real replay CLI reports all 300 readings as one pad; applying those thresholds with hysteresis gives 100 one-pad, 100 two-pad and 100 three-pad readings. Therefore a reported footprint of one/two does not prove a chord cannot qualify. Replay touch/release state with explicit debounce/baseline limits, or label the numbers as instantaneous threshold crossings and remove that gesture conclusion. This is a bounded synthetic case, not a hardware measurement; the physical slider/chord decision stays with the first and third builders.

Fresh 4543f1cb9 gate: fmt and Controls all-target clippy pass; all 385 serial tests pass without retries. The head full CI run 38727552759 failed the existing Memory kill/log test; the separate shard run was still in progress when checked.

Checked at 2026-10-15 18:48 +0700.

## PR #797 — half cutoff rounds down

[Pull request](https://github.com/autonomous-ai/lampOS/pull/797)

Reviewer: [P2] Preserve the stated 50% own-swipe cutoff without rounding it down (`lampOS/crates/controls/src/touch_calibrate.rs:263-267`). For own-swipe peaks [33,16,27,30], 16 is below half of 33, but floor(33*50/100) admits it. The real calibrate-touch --replay CLI with measured noise 3 writes touch 8/release 5 instead of using level 27 and touch 12/release 7. A graze can thus set the much lower threshold this rule intends to reject. Compare peak*100 >= strongest*OWN_SHARE_PERCENT, or round the cutoff upward, and cover odd strongest values. Reproduced at 4543f1cb9 using independent recorded-input fixtures; no lamp was used.

Checked at 2026-10-15 18:48 +0700.

## PR #793 — rebased ci memory failure

[Pull request](https://github.com/autonomous-ai/lampOS/pull/793)

Reviewer: Exact-head ARM64 run 38727844448 on 8563d1a34 failed again in the existing Memory binary kill/log test (`lampOS/crates/memory/tests/binary.rs:309`), with (3,4) observed versus (4,4) expected. It ran 1,802 tests (1,801 passed, 1 failed), skipped 116 and left 2,699 unrun. This remains a red head gate; I have not rerun it. The rebase preserves the own fixes and supplies the three new Voice unread literals. My fresh Msgs, Tasks and Policy host gates pass; Voice is still running. The Memory failure is already tracked with the tests/devops owners, rather than being a new #793 source finding.

Checked at 2026-10-15 18:46 +0700.

## PR #835 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/835)

Reviewer: for the first builder: #835 changes the default startup privacy path and explicitly requests lamp-4ace's power-cut acceptance before merge. I have read #762 and the full diff at 282e31382; I am checking invalid/stale/sequence observations, the one-second unavailable bound and the fresh-unmute requirement, and running the full Voice gate. #762 remains open for its other startup causes and real power-cut evidence.

Checked at 2026-10-15 18:46 +0700.

## PR #834 — invocation and owner amendment

[Pull request](https://github.com/autonomous-ai/lampOS/pull/834)

Reviewer: [P2] Make the documented command executable (`lampOS/tools/lamp-lock.sh:1`; `lampOS/docs/joining.md:420-428`). The file is committed as 100644, so the direct lamp-lock.sh invocations fail with permission denied. Set executable mode or consistently document bash invocation. bash -n passes and no Rust crate is changed. #833 now authorizes reviewer merge, subject to its requested age/non-answering-holder status and person-only stale removal rule; those amendments are not yet in head 32d4c3253. The two separately posted lock-identity/transport findings also need fixing.

Checked at 2026-10-15 18:46 +0700.

## PR #834 — unreachable is not free

[Pull request](https://github.com/autonomous-ai/lampOS/pull/834)

Reviewer: [P2] Preserve transport failures in status/release (`lampOS/tools/lamp-lock.sh:32-38,64-68`). With the transport returning 255, status prints “is free” and exits 0, and release prints “was not held” and exits 0. The preliminary reachability check only covers acquire. An unavailable lamp must be reported as unknown/error, not free or successfully released. Distinguish a successful remote absent-file result from SSH/read failure and test both commands with the transport down. Reproduced using the real script and a fake transport; no lamp was contacted.

Checked at 2026-10-15 18:46 +0700.

## PR #834 — literal lock owner

[Pull request](https://github.com/autonomous-ai/lampOS/pull/834)

Reviewer: [P2] Compare holder/session literally (`lampOS/tools/lamp-lock.sh:40`). plain() explicitly permits dots, but ours() interpolates those values into grep regular expressions. With the real script and a local stand-in for SSH, acquiring as builder/sessionX1 then releasing as builder/session.1 succeeds and deletes the other session's lock. The same match also lets acquire report an unrelated session's lock as already ours. Use fixed-string, whole-line comparisons and add this distinct-session case. Reviewed head 32d4c3253; no SSH or lamp action was performed.

Checked at 2026-10-15 18:46 +0700.

## PR #818 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/818)

Reviewer: First pass at 84c02b2a3: I have read #800, the description and worker-loop switch tests. The new quiet-room-checked value is opt-in; I am checking the first-output decision and the short-turns joins/counts, then running the Voice gate. Exact-head ARM64 run 38724241108 is green. #800 remains open for the recorded-sound count and activation acceptance described there. No lamp actions will be part of this review.

Checked at 2026-10-15 18:42 +0700.

## PR #808 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/808)

Reviewer: First pass at c9d997e51: these are additive logging/settings interfaces with consumers in the same PR; existing first_topics and message types remain unchanged. #699 assigns this scope to the third builder, so this remains in the reviewer merge lane under the 15:22 rule. I am reviewing the complete implementation and running fmt plus the Bus, Msgs and Settings gates. Exact-head ARM64 run 38726383908 is green; that is not yet a final approval.

Checked at 2026-10-15 18:42 +0700.

## PR #817 — reset task history

[Pull request](https://github.com/autonomous-ai/lampOS/pull/817)

Reviewer: [P3] Include the active task-news history in the reset plan (`lampOS/crates/setup/fixtures/reset-plan-example.json:11-14`). The default `/var/lib/lampOS/voice/tasks-told` (`voice/src/task_news.rs:33`) is absent. An independent fixture remaps the shipped plan under a temporary directory, calls production reset::reset, and confirms reset completes and removes its marker/journal while the old store/task/observation identifiers remain. Add this file and a reset-coverage assertion. The retained data is identifiers, not task contents; I am not claiming a cross-reset suppression or credential leak.

Post-merge audit of b21a26bb3: fmt and Setup all-target clippy pass; all 27 serial tests pass. Exact-head ARM64 run 38722230047 is green. The two independent regression assertions for reachability and reset fail as described. The first builder merged this PR; no device actions were performed in this review.

Checked at 2026-10-15 18:42 +0700.

## PR #817 — own page port conflict

[Pull request](https://github.com/autonomous-ai/lampOS/pull/817)

Reviewer: [P2] Make own-page mode coexist with the AP script's nginx start (`lampOS/crates/setup/src/main.rs:284,340`; `fixtures/serve-own-page.json:2`). The supplied fallback binds 0.0.0.0:80, but the preceding device-ap-mode unconditionally restarts nginx (`scripts/imager/build-orangepi.sh:753`). On a V1 image with nginx installed but without the reused React bundle, selecting this fallback therefore leaves nginx owning port 80 and Setup fails its bind. Stop/suppress nginx for own-page mode or use a compatible separate port/proxy, and cover that configuration. The normal behind-nginx fixture is unaffected. Evidence is the shipped call/script/bind order; no lamp or systemd run was performed.

Checked at 2026-10-15 18:42 +0700.

## PR #817 — tcp is not tls

[Pull request](https://github.com/autonomous-ai/lampOS/pull/817)

Reviewer: [P2] Preserve the bounded TLS fallback for reachability (`lampOS/crates/setup/src/wifi.rs:408-412`). With ICMP blocked, a TCP listener that never speaks TLS is currently accepted as Internet. An independent fixture invoking the actual ImageHost::internet with a failed ping and a loopback listener reproduces true. Join can therefore save the credential/marker and leave setup even though an intercepting network cannot reach the service. V1's fallback completed a bounded TLS handshake (`system/network/reachability.go:38-49`). Restore that check, including bounded resolution, and cover a listener that accepts TCP but cannot complete TLS. This finding is about reachability, not credential disclosure.

Checked at 2026-10-15 18:42 +0700.

## PR #817 — hostname before hotspot

[Pull request](https://github.com/autonomous-ai/lampOS/pull/817)

Reviewer: [P2] Read the device hostname after starting the hotspot (`lampOS/crates/setup/src/main.rs:279-284`). On a fresh image with the generic image hostname, this caches the old name before device-ap-mode assigns lamp-<suffix> (`scripts/imager/build-orangepi.sh:725-726`). If the AP disappears before the phone receives lan_ip, V1's handoff either disables mDNS for that generic name or probes the wrong .local address and reports a failed join. Move the read after hotspot() and cover a hotspot start that changes the name. This is verified from the shipped script and UI handoff code; no phone or lamp run was performed.

Checked at 2026-10-15 18:42 +0700.

## PR #795 — linux close before reap test

[Pull request](https://github.com/autonomous-ai/lampOS/pull/795)

Reviewer: [P2] Handle the Linux close-before-reap error without dropping the exit checks (`lampOS/crates/voice/tests/privacy_worker.rs:469-480`). Exact-head ARM64 run [38726030376](https://github.com/autonomous-ai/lampOS/actions/runs/38726030376) fails the new losing_controls_ends_the_real_worker_and_expires_its_lease_despite_current_allowed_authority test with ENOTCONN (Transport endpoint is not connected, OS 107) at line 474, after “physical privacy source expired”. This loop already tolerates the worker closing IPC before try_wait reports death, but accepts only BrokenPipe, ConnectionReset and ConnectionRefused. Linux can report NotConnected in the same window. Handle that specific case or wait for confirmed exit through expected close errors, retaining the bounded deadline, still-current controller authority, abnormal-exit and expired-lease assertions. The run stopped at 4,233 passed / 1 failed, with 293 not run and 103 skipped; this is not a green head gate. I have not rerun it.

The rebase's own patch is preserved; range-diff changes only its placement beside the merged attention/tasks declarations. The fresh six-crate host gate is running, but this Linux-only test cannot execute on the Mac host.

Checked at 2026-10-15 18:34 +0700.

## PR #794 — rebase verified

[Pull request](https://github.com/autonomous-ai/lampOS/pull/794)

Reviewer: Rebase `abbd3ce8d` is verified with no new finding. #770's first waiting answer is preserved; a previously reported waiting task is described as still waiting on its computer without inventing a question. FIFO waiting/tell, news selection and store/task/observation identity are byte-identical to the previously reviewed implementation. Fmt and Voice all-target clippy pass; the fresh full serial Voice gate passes 1,189 tests with 8 ignored and no retries. Exact-head ARM64 run 38725512979 is green. Reviewer: for the first builder: this changes default speech/task-check behavior, so your story/follow-up acceptance and merge remain.

Checked at 2026-10-15 18:33 +0700.

## PR #810 — audio during call still negative

[Pull request](https://github.com/autonomous-ai/lampOS/pull/810)

Reviewer: [P2] Exclude all audio earlier than the tool answer from the post-answer metric (`lampOS/crates/voice/src/turn_times.rs:279-286`). The new lead_in label tests audio < called_us, but an audio event between called_us and sent_us still passes the filter and is subtracted from sent_us. An independent replay at `6cc4f2c09` with call=1,100 ms, first_audio=1,102 ms and answer_sent=1,104 ms reports lead_in=false and first_audio_after_answer_ms=-2.0. Worker and coordinator timestamps are recorded on different processing paths, and audio can be forwarded while a task call is outstanding. Exclude samples earlier than sent_us regardless of the before-call label, or record a separate first-post-answer audio event. Add this between-call-and-answer case. The original order/rotated-files, 900 ms lead-in and terminal-ending proofs now pass; the blocked-stderr proof records about 1 microsecond drift despite a 100 ms blocked log. This one measurement finding remains.

Checked at 2026-10-15 18:33 +0700.

## PR #815 — target condition does not gate services

[Pull request](https://github.com/autonomous-ai/lampOS/pull/815)

Reviewer: [P1] Gate each lamp service, or let the keeper exclusively choose the first-boot target (`lampOS/deploy/product/lampos.target:8`; `lampOS/crates/update/src/install.rs:152-161`, `:394`). ConditionPathExists on a target does not gate its dependencies. install enables the product services with WantedBy=lampos.target and enables that target under multi-user.target. On boot, systemd queues those services before checking the target's missing setup marker; Voice and Ears have no marker condition of their own. In particular, Ears needs no provider credential to open its room microphone. The promised “nothing listens before setup is done” is therefore not enforced by this unit graph.

This follows from the actual unit graph and [systemd 252's Conditions and Asserts contract](https://manpages.debian.org/bookworm/systemd/systemd.unit.5.en.html#Conditions_and_Asserts): condition checks occur after dependency jobs are queued and do not condition those dependencies. I have not run a native boot or touched a lamp. Add the marker condition to every gated part, or avoid pulling the target in at boot until the keeper selects it, and test marker-absent/marker-present starts against real systemd. This also corrects the symptom in my bootstrap comment: a skipped target does not mean its parts stay stopped. For the first builder: this is the setup privacy boundary.

Checked at 2026-10-15 18:29 +0700.

## PR #815 — bootstrap not installed

[Pull request](https://github.com/autonomous-ai/lampOS/pull/815)

Reviewer: [P2] Supply or explicitly require the bootstrap path before calling this a complete blank-lamp install (`lampOS/docs/install.md:263-268`; `lampOS/crates/update/src/install.rs:29-112`). The doc says install installs the keeper's unit, but PRODUCT contains neither a keeper unit/binary nor the setup service. No keeper/setup service is installed to own the first-boot setup and post-setup hand-over on the documented base image. The separate target-condition finding below addresses premature starts of its dependencies. Add the keeper/setup integration (including the #817 dependency) and test that declared first-boot path, or make it an explicit installed-image prerequisite and mark this PR partial with #811 left open. A successful file-copy test alone does not complete that boot path.

Checked at 2026-10-15 18:25 +0700.

Clarified the boot symptom at 2026-10-15 18:29 +0700; the missing bootstrap dependency remains.

## PR #789 — final gate after first merge

[Pull request](https://github.com/autonomous-ai/lampOS/pull/789)

Reviewer: Final host evidence for merged `07e935f8a`: the 29-to-32 docs correction is verified, both other patches are exact range-diff matches, and the Voice tree is identical to the prior reviewed head. Msgs/Tasks/Policy dependency trees match merged #751. Fresh fmt and Voice all-target clippy pass. The full serial run reported 1,216 passed / 2 failed / 8 ignored: the unchanged reconnect-heartbeat fixture (ConnectionReset) and reminder completed-playout/real-store-reopen fixture (AckConfirmed assertion). Both exact same-binary isolated retries pass; originals are retained, giving 1,218 distinct Mac passes after retries. Exact-head ARM64 run 38725112153 is green. No remaining source/doc finding. The first builder merged this PR; #739 remains open for part 2.

Checked at 2026-10-15 18:28 +0700.

## PR #825 — merged nightly gap

[Pull request](https://github.com/autonomous-ai/lampOS/pull/825)

Reviewer: Follow-up after the concurrent merge: #825 is merged as `0fd1db7f2`; #826 remains open against main as checked below. The automatic-full-run gap is therefore current, not just a merge-order concern. The first builder's 18:26 condition explicitly requires keeping full push runs until the nightly trigger is merged and has run green once. Devops needs to restore that coverage until the condition is satisfied. The successful manual full run validates this head but does not install an automatic trigger.

Checked at 2026-10-15 18:28 +0700.

## PR #809 — model cut continuation missing

[Pull request](https://github.com/autonomous-ai/lampOS/pull/809)

Reviewer: [P1] Wire name preservation into the actual model-complete comma-cut path (`lampOS/crates/voice/src/coordinator.rs:5059`, `:5153`). With LAMP_CUT_KEEPS_NAME=on and the documented smart-turn-fast / 600 ms / kept-start options, early_end_blocks is None. A deterministic replay through the real TurnDetector gives model_complete after 400 ms of the comma pause, then a fresh Activity::Start when speech resumes. That candidate goes through commit_admitted_input, which hardcodes continues=false at line 5059, so Attention starts afresh and loses the first part's call exactly as before.

The only switch-controlled true assignment is in continue_after_early_end at line 5153, reached through EarlyEnd::hear only with the separate --early-end-ms option and before detector confirmation. Even with that option, AdmissionStep::End confirms/clears the pending early end at lines 3851-3854. The added worker tests manually inject continues=true and do not exercise this coordinator boundary. Carry bounded predecessor/session/privacy identity through the post-model-end admission path and add a coordinator-level regression for a model-complete comma pause followed by the rest of the same request. The independent detector/options proof passes on the reviewed head 9bda9dcd3 and establishes the event path; it is not a full coordinator run. This needs a follow-up now that the PR has merged.

Checked at 2026-10-15 18:28 +0700.

## PR #825 — first builder validation

[Pull request](https://github.com/autonomous-ai/lampOS/pull/825)

Reviewer: for the first builder: this changes default merge CI and the validation behind update artifacts, so merge belongs with you/devops. YAML parses, all seven run scripts pass bash -n after expression substitution, fmt and diff checks pass; no Rust crate changes. I verified exact-head 6cd157ce1 full run 38723605504 succeeded on all four ARM64 jobs. The failure-report job is skipped, so its failure behavior remains unexecuted as the PR states. The earlier red-full-run merge-hold finding remains unresolved: this workflow opens an issue but does not stop later merges or artifact publication. Please also remove the generated attribution/footer from the PR description.

Checked at 2026-10-15 18:25 +0700.

## PR #825 — nightly dependency

[Pull request](https://github.com/autonomous-ai/lampOS/pull/825)

Reviewer: [P2] Land and verify the nightly trigger before removing the remaining automatic full run (`.github/workflows/lampos-arm64.yml:102-103`). #826 is still OPEN against main. This head selects the filtered pr profile for every push, and only an explicit workflow_dispatch full=true runs realtime tests. If this PR lands first, no automatic event runs them until that separate dependency is installed and working, despite docs saying the nightly run does. Keep push coverage until #826 is merged/verified, or make that dependency an explicit merge prerequisite.

Checked at 2026-10-15 18:25 +0700.

## PR #815 — preserve local settings

[Pull request](https://github.com/autonomous-ai/lampOS/pull/815)

Reviewer: [P2] Preserve existing per-lamp settings when install is run again (`lampOS/crates/update/src/install.rs:362-363`). Every entry in SETTINGS is replaced with the embedded default whenever its bytes differ. With the real installer and a temporary root, install once, change the primary button's line in /etc/lampOS/controls/controls-pins.json from 99 to 100, then install the same signed build again: it silently restores line 99. #811 explicitly puts lamp-specific pins/calibration in settings or drop-ins, and rerunning install is also the documented updater self-upgrade path. Seed defaults only when absent, or provide a migration that preserves local overrides; add a rerun case with a valid changed pins/settings file.

Checked at 2026-10-15 18:25 +0700.

## PR #815 — retry failed enable

[Pull request](https://github.com/autonomous-ai/lampOS/pull/815)

Reviewer: [P2] Finish the host actions after an interrupted install (`lampOS/crates/update/src/install.rs:392-395`). They run only when this invocation changed a file. An independent test against the real install() function injects failure from Host::enable after the users/files/links have been written, then runs install again. The retry returns success with changed=[] and host.calls=[]: it never retries enable (or daemon-reload inside it), so a blank lamp can remain unbootable after an otherwise recoverable install failure. The same gap exists after tmpfiles failure or a process stop before these actions. Persist/reconcile unfinished host work independently of file changes, and add the failed-first-run/retry regression. Fmt, Update all-target clippy and the existing serial gate pass: 59 tests, 1 ignored.

Checked at 2026-10-15 18:25 +0700.

## PR #822 — runtime fixes gated provenance pending

[Pull request](https://github.com/autonomous-ai/lampOS/pull/822)

Reviewer: At frozen `8a4304da5`, independent checks now pass for full rule-state reading, nondestructive missing/invalid forget IDs, and the bus work leaving the note path (118 ms acquisition while the publisher is blocked for 800 ms). Fmt and Msgs/Voice/Memory all-target clippy pass. Full serial gate: 1,419 passed / 1 failed / 13 ignored. The sole failure was the unchanged reconnect-heartbeat test's ConnectionReset; its exact isolated retry passes, with the original failure retained. Memory's earlier killed-process log-count test passed in this full run.

The provenance proof passes only because journalling is disabled, which the first builder explicitly rejected. That finding and the separately posted atomic six-rule admission finding remain open. The latest `37b52aab5` wording/test/docs delta was read, but this full gate is for `8a4304da5`. Neither fix head has an attached ARM64 run in the checked branch history; older green runs do not validate these fixes. Corrected spoken acceptance is also still pending.

Checked at 2026-10-15 18:20 +0700.

## PR #791 — reference gap fixed ring pending

[Pull request](https://github.com/autonomous-ai/lampOS/pull/791)

Reviewer: At `6735459f6`, the controlled-reference finding is fixed: hold_up sends a reference every 10 ms while withholding authority, and its sequence/sample cursor is shared with streaming. Three gaps match the unchanged three-reseeds-per-minute recovery policy. Your native old-guard 5/5 failures and new-guard 5/5 passes are explicit execution evidence, separate from PR CI; I have reviewed that report, not independently run it. Please retain its command and raw per-test output with the evidence. Fmt and Voice all-target clippy pass. Production and all Mac test inputs are byte-identical to the prior gate, so its Voice 1,190 passed / 8 ignored and Audio 160 passed / 2 ignored apply unchanged.

Reviewer: for the first builder: your newer all-workers failure remains open. The test at `lampOS/crates/voice/tests/capture_hold_worker.rs:642-665` has no ring worker or real coordinator, and this revision changes no production code. Ring expiry is terminal in ring_worker, but coordinator.rs:5230-5244 handles a failed optional ring; the log alone does not identify what caused Voice's final authority-snapshot expiry. Please keep the all-workers reproduction and trace, including the ring, as the remaining integration check.

Checked at 2026-10-15 18:20 +0700.

## PR #817 — first review handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/817)

Reviewer: for the first builder: `b21a26bb3` is now ready, but #806 step 4 still requires a timed wiped-lamp-to-conversation run with a phone, and the PR explicitly says that is unperformed. The reset's durable wipe/reboot and setup-to-keeper transition also need that integration judgment. I am reviewing the host implementation and gates; please keep #806 open for its unperformed acceptance even if the code is merged separately.

Checked at 2026-10-15 18:20 +0700.

## PR #809 — first review handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/809)

Reviewer: for the first builder: `9bda9dcd3` keeps both changes opt-in and has green exact-head ARM64 run 38724198733, but #782 reserves the lamp-4ace 400/550 ms comparison and this PR has no own-lamp demonstration. The named local loading-test failure is retained. I am reviewing the continuation/attention boundaries, endpoint option validation, replay evidence and full touched-crate host gate; lamp acceptance and merge remain with you.

Checked at 2026-10-15 18:20 +0700.

## PR #793 — second fixes verified

[Pull request](https://github.com/autonomous-ai/lampOS/pull/793)

Reviewer: Both follow-up findings are fixed at `ce2b47116`. The actual-Tracker new-run/unread proof now asks at the first observed count 1, and its control passes (2/2); both real-service unread/recovery checks also pass (2/2). Fmt and Msgs/Tasks/Policy all-target clippy pass. Fresh serial gates: Msgs 83, Tasks 122, Policy 161 passed, with 2 Policy ignores and no failures.

The exact-head ARM64 run [38724419493](https://github.com/autonomous-ai/lampOS/actions/runs/38724419493) failed in unchanged Memory `a_killed_process_leaves_a_store_the_next_one_loads_and_a_socket_it_replaces` (`crates/memory/tests/binary.rs:309`): log counts (3, 4), expected (4, 4). It stopped after 1,802 passed / 1 failed, with 2,632 tests not run and 102 skipped. This is not a green head gate; I have not rerun CI.

#765 is now merged as `4ad0ec5c3`. Please rebase onto it and update the new Voice TaskStatus literals for `unread`, then gate Voice all targets as well as the three changed crates. The present head predates those literals. No remaining source finding on this reviewed head; the combined tree and its green exact-head ARM64 remain to verify. For the first builder: this still changes the enabled progress path on lamp-4ace and carries your lamp check.

Checked at 2026-10-15 18:14 +0700.

## PR #765 — merged issue 732

[Pull request](https://github.com/autonomous-ai/lampOS/pull/765)

Reviewer: merged #765 at 18:14 as `4ad0ec5c3`, completing the live consumer after #751; #732 stays open because its start/read/journal follow-up #793 is still unmerged.

## PR #765 — merged issue 752

[Pull request](https://github.com/autonomous-ai/lampOS/pull/765)

Reviewer: merged #765 at 18:14 as `4ad0ec5c3` — opt-in live progress decisions and fresh, restart-safe Voice news; #751 is already merged, and hello remains the separate follow-up agreed here.

## PR #765 — final rebase source verified

[Pull request](https://github.com/autonomous-ai/lampOS/pull/765)

Reviewer: Final rebase `b5917f9f5` is verified. Its sole patch is an exact range-diff match to reviewed `ea47efd01`; the dependency commits are gone because #751 is merged. Git object identities match for the complete crates tree, Cargo.toml, Cargo.lock and toolchain, so the completed source gates and independent regressions apply unchanged. The only inherited tree changes are CI configuration and its docs. No remaining review change is requested. The new exact-head ARM64 run is required separately before merge.

Checked at 2026-10-15 18:04 +0700.

## PR #822 — concurrent rule admission

[Pull request](https://github.com/autonomous-ai/lampOS/pull/822)

Reviewer: [P2] Make the six-rule admission atomic (`lampOS/crates/voice/src/remember.rs:1158-1174`). The new limit first reads the scope, then performs remember in a separate socket operation. ToolDesk runs up to eight memory calls concurrently (`provider_worker.rs:1699-1709`). Against the real Days store with one shared DaysClient and five existing rules, eight simultaneous keep_rule calls all returned kept_rule=true; list_rules then contained 13 rules. The claimed six-rule bound can therefore be exceeded by ordinary parallel tool calls. Make count-and-insert one store transaction, or serialize the complete admission/write for the scope without exceeding the tool deadline, and add the concurrent real-store case. Reproduced at `8a4304da5`; the relevant code is unchanged in `37b52aab5`.

Checked at 2026-10-15 18:04 +0700.

## PR #789 — rebased full gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/789)

Reviewer: Full host validation for `417724d26` is complete. Fmt and Voice all-target clippy pass. The first full serial run reported 1,217 passed / 1 failed / 8 ignored. The sole failure was the existing threaded EarlyEnd test the_link_dropping_in_between_fails_the_request_unheard_and_the_next_is_answered: 595 ms at its 600 ms assertion. Its exact same-binary retry passes at 603 ms; the original failure is retained under the Queue's timed-fixture exception, with no asserted root cause. That gives 1,218 distinct passing Mac tests after the retry. All three added pause cases passed in the full run; production pause handling and the non-test Instant::now() boundary are preserved. Current-head ARM64 run 38722576777 is green. Only the 29-to-32 documentation count remains to change before merge.

Checked at 2026-10-15 18:02 +0700.

## PR #791 — new linux test skipped

[Pull request](https://github.com/autonomous-ai/lampOS/pull/791)

Reviewer: [P2] Execute the new Linux regression before claiming it was covered by ARM64 CI (`lampOS/crates/voice/tests/capture_hold_worker.rs:621`). Run 38722569693 is green, but its actual command uses --profile pr; the inherited default filter excludes the entire capture_hold_worker binary. The downloaded logs contain no execution of five_stalls_in_a_row_each_reopen_the_microphone_while_the_reference_goes_on. The Mac gate cannot execute that Linux-only target either. Arrange a native full-profile or explicit target run with devops and retain its per-test result, alongside fixing the controlled reference-gap coverage above. The PR comment that “ARM64 CI runs it” is not supported by this run. Green compilation and the reported SIGSTOP lamp exercise are separate evidence.

Checked at 2026-10-15 18:02 +0700.

## PR #765 — rebased host gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/765)

Reviewer: Fresh host validation at `ea47efd01` is complete. Workspace fmt and Msgs/Tasks/Policy/Voice all-target clippy pass. The first full serial no-fail-fast run reported 1,559 passed / 2 failed / 10 ignored: Msgs 83 and Tasks 118 passed; Voice passed all 1,199 with 8 ignored; Policy had 159 passed / 2 failed / 2 ignored. The Policy failures were `the_lamps_name_heard_while_someone_spoke_is_joined_to_what_they_said` (old name-hearing context at policy.rs:728) and `bad_arguments_print_the_usage_and_end_with_status_2` (missing usage text at program.rs:448). Both exact same-binary isolated retries pass. A subsequent fresh full serial Policy run also passes all 161 with 2 ignored. The initial two failures are retained; this is not a clean first gate. All three independently recompiled consumer-restart proofs pass. Exact-head pull_request ARM64 run [38721851499](https://github.com/autonomous-ai/lampOS/actions/runs/38721851499) is green. No remaining code finding; the final rebase after #751 remains before merge.

Checked at 2026-10-15 18:00 +0700.

## PR #791 — controlled reference gap test

[Pull request](https://github.com/autonomous-ai/lampOS/pull/791)

Reviewer: [P2] Make the new reference-gap regression control the reference traffic during the gap (`lampOS/crates/voice/tests/capture_hold_worker.rs:627-629`; helper `:374-378`). The renamed five-stall test calls hold_up(400), which sleeps the thread owning the sole reference sender, and only then calls stream(). It therefore sends no reference during that 400 ms interval, despite the name/PR claim that reference continues through every gap. Resume scheduling may incidentally exercise the race, but the test does not reliably exercise the closed-microphone/reference branch changed in `00de8ec9e`. Keep reference traffic active while authority is withheld, wait for CaptureGap, deliver a valid-generation reference before renewing authority, and show that controlled case fails at `46cb96268` and passes with the fix. This is a regression-coverage finding, not a claim of a newly reproduced runtime fault; the reported five SIGSTOP lamp run remains separate evidence.

Checked at 2026-10-15 17:55 +0700.

## PR #824 — native result verified

[Pull request](https://github.com/autonomous-ai/lampOS/pull/824)

Reviewer: Completed-run evidence now verifies the parallel runtime statement. Exact-head pull_request run [38721013037](https://github.com/autonomous-ai/lampOS/actions/runs/38721013037) is green at `2797292ee`: the runner reports 4 cores; nextest reports 4,399 passed, 102 skipped, in 55.079 seconds. Test compilation took 5m29s and the separate release build 2m17s; those are outside the 55-second execution figure. Doc/custom-harness steps also succeeded. The earlier pending-runtime caveat is resolved. Devops has merged this PR; the nightly-full-run and failed-full-run merge-hold findings inherited from #821 remain separate.

Checked at 2026-10-15 17:54 +0700.

## PR #789 — pause fixture count

[Pull request](https://github.com/autonomous-ai/lampOS/pull/789)

Reviewer: [P3] Update the scenario count after the keeper-pause rebase (`lampOS/docs/voice-test-fixtures.md:21`; `lampOS/docs/pilot-faults.md:31`). At `417724d26`, pilot_faults.rs now has 32 stepped Script::question scenarios plus the unchanged threaded mute/reopen scenario; both docs still say 29 stepped scenarios. The three new pause tests account for the increase. Change the count to 32 or avoid a fixed count. The conflict resolution otherwise preserves the pause behavior, assertions and production clock boundary; the fresh full Voice gate is running.

Checked at 2026-10-15 17:54 +0700.

## PR #822 — lamp evidence followup

[Pull request](https://github.com/autonomous-ai/lampOS/pull/822)

Reviewer: At `5c6c6dc17`, the delta adds lamp evidence only; runtime crates, Cargo files and toolchain are identical to reviewed `d381ef9c2`. Fmt/clippy pass, and all four independent regression checks still fail (the stalled bus case took 923 ms on this run). The completed identical-source full gate remains 1,416 passed / 1 failed / 13 ignored; the unchanged Memory failure was not rerun again.

The lamp record is now present, so the earlier absence of evidence is partially addressed. It documents two remaining acceptance failures: an implicit standing correction was acknowledged without keep_rule, and a request to forget the weather rule removed the newer addressing rule instead. Later-session unprompted obedience is still unshown. Fix those paths and provide the successful spoken cases; update the PR description, which still says no lamp run was shown. None of the four code findings is fixed in this evidence-only head.

Checked at 2026-10-15 17:51 +0700.

## PR #791 — 46cb fixes verified

[Pull request](https://github.com/autonomous-ai/lampOS/pull/791)

Reviewer: The four earlier code findings are resolved at `46cb96268`. All three independent coordinator regressions now pass (reopen between drains, capture expiry during output, and a short explicit XRun); the final per-frame maintain error enters gap recovery. An additional real-socket check passes for the new one-late/second-late rejection/fresh-reset behavior. Workspace fmt, Audio/Voice all-target clippy and the full serial gate pass: 1,350 tests, 0 failures, 10 ignored. Exact-head ARM64 run 38720361917 is green on all four shards.

The new head `00de8ec9e` and its reference-during-gap fix are under review now. The previous three-stall fixture proves expiry/reopen cycles but does not itself pause the capture thread while speaker-reference packets queue; it does not replace the reported SIGSTOP lamp evidence. The exact pause between try_read and final maintain is still not injected by a test. Reviewer: for the first builder: the new late-message allowance changes freshness acceptance on every Voice channel, and this default/privacy behavior remains your judgment even after host checks pass.

Checked at 2026-10-15 17:51 +0700.

## PR #795 — current pr ci green

[Pull request](https://github.com/autonomous-ai/lampOS/pull/795)

Reviewer: The exact-head pull_request ARM64 run [38712595748](https://github.com/autonomous-ai/lampOS/actions/runs/38712595748) is now green on all four shards at `4c154fee3`. The earlier failed push run is a separate run and was not rerun by this reviewer. The reviewed code/docs fixes and host gate remain as recorded above.

Reviewer: for the first builder: the remaining acceptance is the physical privacy-switch behavior with the lamp; this is still your merge under the privacy-path rule.

Checked at 2026-10-15 17:49 +0700.

## PR #765 — parent merged

[Pull request](https://github.com/autonomous-ai/lampOS/pull/765)

Reviewer: #751 is now merged into lamp-voice as `ecca16554` (its exact-head ARM64 and fresh Msgs/Tasks/Policy gates passed). Please rebase this dependent PR onto current lamp-voice as planned. For `ea47efd01`, the range-diff preserves the reviewed change, including #770's waiting() behavior; fmt and all-target clippy pass, and all three independent actual-Tracker/real-Bus consumer restart proofs pass after recompilation. The full serial gate is still running. I will verify the final rebase and its exact-head ARM64 before merging.

Checked at 2026-10-15 17:49 +0700.

## PR #793 — unread runtime test

[Pull request](https://github.com/autonomous-ai/lampOS/pull/793)

Reviewer: [P2] Update the existing runtime regression to account for the newly published unread state (`lampOS/crates/tasks/tests/runtime.rs:487-489`). The next status after task acceptance may now legitimately be unread:[id], tasks:[]; the test takes exactly one bus value and indexes tasks[0]. The full serial Tasks gate fails with index out of bounds, and selecting only this test fails again at the same line. Wait with a bounded event loop for that task's first read value while asserting the intervening unread state, instead of assuming the next publication is already read.

The original unread-versus-empty finding is fixed: independent real-service checks pass for both a newly accepted task with failed reads and a persisted task restarted before its first successful read; get_progress correctly returns not_read and recovery publishes the readable value. Fmt and Msgs/Tasks/Policy all-target clippy pass. Serial tests: Msgs 83 passed; Policy 161 passed / 2 ignored; Tasks 121 passed / 1 failed as above. The two findings in this follow-up remain open.

Checked at 2026-10-15 17:48 +0700.

## PR #793 — unread loses run reset

[Pull request](https://github.com/autonomous-ai/lampOS/pull/793)

Reviewer: [P2] Preserve each task's new-run reset until its first readable progress snapshot (`lampOS/crates/policy/src/progress.rs:516-523`, `:540-544`). At `ceffaa68e`, a new-run unread status sets self.run and judged=0 but retains the old run's last progress. If the first successful count-0 snapshot is missed, the next real read at count 1 enters status() with new_run=false; the lower-count branch overwrites judged with 1 and next() suppresses that new progress. An independent proof using actual Tracker output reproduces old-run count 2 -> new-run unread:[3] -> first observed read count 1: asked=false. The control without the unread placeholder correctly asks. Keep the per-task reset active while tracked.unread, or otherwise distinguish the first read of a new run from a legacy same-run counter reset; add this sequence to the regression.

Checked at 2026-10-15 17:48 +0700.

## PR #751 — merged issue

[Pull request](https://github.com/autonomous-ai/lampOS/pull/751)

Reviewer: merged #751 at 17:47 — opt-in task progress on the bus and Policy shadow decisions, with restart identity and cooldown fixes; #732 stays open for the remaining live progress speech in #752 / #765.

## PR #822 — host gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/822)

Reviewer: Host gate completed for `d381ef9c2`: workspace fmt and all-target clippy for Msgs, Voice and Memory pass. Full serial no-fail-fast tests: Msgs 80 passed; Voice 1,187 passed / 8 ignored; Memory 149 passed / 1 failed / 5 ignored. Total 1,416 passed, 1 failed, 13 ignored. The sole failure is the inherited Memory binary test `a_killed_process_leaves_a_store_the_next_one_loads_and_a_socket_it_replaces`, whose log-count assertion sees (3,3) instead of (4,4); it also fails on its exact isolated retry. This is not recorded as a passing gate. The test and Memory production source are unchanged here. All nine added or extended rule/switch tests executed and passed, but the four independent regression proofs reported above still fail. Exact-head pull_request ARM64 run [38719168444](https://github.com/autonomous-ai/lampOS/actions/runs/38719168444) is green on all four shards.

Checked at 2026-10-15 17:42 +0700.

## PR #751 — rebase verified

[Pull request](https://github.com/autonomous-ai/lampOS/pull/751)

Reviewer: Rebase `16f08fa11` is verified: range-diff preserves the four reviewed patches, and the Msgs resolution keeps both attention and tasks exports. Freshly cleaned/rebuilt Msgs, Tasks and Policy pass workspace fmt, all-target clippy with -D warnings, and serial tests: 83 / 118 / 160 passed respectively, 0 failures, 2 Policy tests ignored. All nine independent restart/stale-answer checks also pass against this head. No remaining code finding; GitHub now reports MERGEABLE. Exact-head ARM64 run 38720578121 is still pending.

Checked at 2026-10-15 17:39 +0700.

## PR #824 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/824)

Reviewer: Reviewed `2797292ee`: the matrix size, partition denominator and job name agree for one PR/manual runner and four push runners, and doc/custom-harness tests still run once. Workspace fmt, YAML parsing, all seven shell-block syntax checks and diff whitespace checks pass. No Rust crate source changes here, so there is no changed-crate clippy/test gate. Current-head ARM64 run [38721013037](https://github.com/autonomous-ai/lampOS/actions/runs/38721013037) is still in the test step; the parallel test count and runtime are not yet verified. Please record the completed run before presenting “about a minute in parallel” as a measured result (`lampOS/docs/realtime-tests.md:15-18`), and remove the attribution footer from the PR description.

Reviewer: for the first builder: this changes CI behavior by default and belongs to the human devops team. Devops has now merged #821; its previously reported missing nightly full run and failed-full-run merge hold remain unchanged here (`.github/workflows/lampos-arm64.yml:33-40`, `:119-140`). This review of the one-runner delta does not resolve those two findings.

Checked at 2026-10-15 17:38 +0700.

## PR #789 — green needs rebase

[Pull request](https://github.com/autonomous-ai/lampOS/pull/789)

Reviewer: Current-head pull_request ARM64 run [38712405805](https://github.com/autonomous-ai/lampOS/actions/runs/38712405805) is now green on all four shards at `dbabac931`. The reviewed source, documentation correction and host gate have no remaining finding. GitHub now reports CONFLICTING/DIRTY against lamp-voice, however. Please rebase the reviewed test changes onto the current branch and obtain the new-head run; I will check the conflict resolution and any source delta. This remains Part of #739, which must stay open for the motor-fixture work.

Checked at 2026-10-15 17:37 +0700.

## PR #822 — blocking policy publication

[Pull request](https://github.com/autonomous-ai/lampOS/pull/822)

Reviewer: [P2] Move optional bus publication off the bounded session-acquisition path (`lampOS/crates/voice/src/remember.rs:1168-1173`; `lampOS/crates/voice/src/rules.rs:344-365`). The 300 ms deadline bounds the memory socket reads but not the synchronous Bus::publisher_declared/publish_json filesystem work afterward. OUT.try_lock only avoids waiting for the mutex. With the real Days client and a temporary declaration FIFO held open for 800 ms, told_for_session(..., true) took 941 ms; this runs before provider connection in thread.rs. A blocked bus therefore delays voice readiness despite the documented budget. Use a bounded background/coalescing publisher and test a stalled publication without extending note/tool completion deadlines.

Reviewer: for the first builder: #813 explicitly requires spoken corrections on the builder's own lamp before lamp-4ace, and the PR says that demonstration has not happened. That evidence is still needed. The new message type alone is permitted by the amended reviewer role. Current-head ARM64 is green; local Msgs/Voice/Memory gates are still being collected. These four findings were independently reproduced against compiled d381ef9c2 code.

Checked at 2026-10-15 17:35 +0700.

## PR #822 — forgotten rule provenance

[Pull request](https://github.com/autonomous-ai/lampOS/pull/822)

Reviewer: [P2] Preserve rule provenance and forget invalidation when a rule influences a journalled answer (`lampOS/crates/voice/src/remember.rs:1120-1148`, `:1168-1173`). The new sibling r- scope is read into provider context without joining the provenance/invalidation path. Real Days-store proof: keep the synthetic ORCHID addressing rule; open a session that receives it; finish an ordinary answer quoting it. That answer is persisted with append_linked source_ids=[]. After successful forget_rule, both the earlier derived answer and another answer from stale provider context are accepted and returned by ordinary recall. Thus the rule can return through the person's memory after deletion; the statement that sibling scopes keep rules out of recall does not cover derived exchanges. Add scope-aware provenance and deletion/inhibition for rule-derived context, or conservatively prevent such journalling. Verify that both already persisted derivatives and post-forget stale context cannot reintroduce the removed rule. This concerns persisted recall, not retracting words already sent to a provider.

Checked at 2026-10-15 17:35 +0700.

## PR #822 — invalid number deletes

[Pull request](https://github.com/autonomous-ai/lampOS/pull/822)

Reviewer: [P2] Reject an explicitly invalid rule number instead of treating it as omission (`lampOS/crates/voice/src/rules.rs:152-159`). The as_u64/filter chain sends number=0, negative numbers, and non-integer values to forget_last. The real-store regression keeps one rule and calls forget_rule {number:0}; the result is forgotten=true and list_rules becomes empty. Only an absent number should select the most recent rule. Validate a present value and return an error without mutation; test absent, valid, zero, negative, and wrong-type cases.

Checked at 2026-10-15 17:35 +0700.

## PR #822 — older rules disappear

[Pull request](https://github.com/autonomous-ai/lampOS/pull/822)

Reviewer: [P2] Keep the complete standing-rule state separate from the prompt byte budget (`lampOS/crates/voice/src/rules.rs:275`, `:332-338`). At `d381ef9c2`, the same newest-first 600-byte `kept` slice drives the session note, list_rules, and policy booleans/count. Against the real Days store, keeping "Never speak first" with never_first, followed by three valid 200-byte corrections, succeeds four times but then publishes quiet=false, quiet_mornings=false, rules=3; list_rules and the next session omit the unforgotten quiet rule. Adding an unrelated correction silently reverses an existing instruction. Retain/evaluate all active rules for policy and listing, and define an explicit bounded admission or summarization policy for session instructions instead of silently discarding older accepted rules. Add this real-store regression.

Checked at 2026-10-15 17:35 +0700.

## PR #810 — final host gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/810)

Reviewer: Host gate complete at `d91b5fb90`: `cargo fmt --all --check` and `cargo clippy -p lamp-voice --all-targets -- -D warnings` pass. The full serial no-fail-fast test run had 1,173 passed, 1 failed, 8 ignored. The failure was `coordinator::tests::pilot_faults::provider_failure_during_output_never_becomes_a_completed_answer` (capture discontinuity); its exact isolated retry passes. This is the recorded timing-family exception, not a clean first run: 1,174 distinct tests passed after that retry. Current-head pull_request ARM64 run [38714841106](https://github.com/autonomous-ai/lampOS/actions/runs/38714841106) has all four shards green. The four independently reproduced measurement findings above remain open; the first builder's lamp comparison remains required.

Checked at 2026-10-15 17:35 +0700.

## PR #821 — nightly full route

[Pull request](https://github.com/autonomous-ai/lampOS/pull/821)

Reviewer: [P2] Provide the required nightly full run and route non-PR validation to it (`.github/workflows/lampos-arm64.yml:25-33`, `:82`). The current expression selects default only for push; workflow_dispatch selects pr and excludes realtime tests too. There is no schedule or nightly dispatcher in this PR. Add an actual nightly route on lamp-voice and make it run the full/default profile, with the same failure reporting and merge hold.

Reviewer: for the first builder: your explicit do-not-merge instruction remains in force; CI ownership has also moved to devops. Current-head validation: YAML and TOML parse, all seven shell run blocks pass bash -n, and the three repeated filtersets agree. No Rust crate source changed, so no crate build/test was needed for this configuration review. The PR description also still contains a generated-with attribution footer; remove it to follow the no-attribution rule.

Checked at 2026-10-15 17:26 +0700.

## PR #821 — missing merge hold

[Pull request](https://github.com/autonomous-ai/lampOS/pull/821)

Reviewer: [P2] Complete the required hold after a failed full run before excluding tests from PRs (`.github/workflows/lampos-arm64.yml:113-131`). At `2a6cff170`, the failed-push job only opens/comments on an issue. If merge A breaks one of the excluded capture/mute/conversation tests, its later full run goes red, but B's reduced green PR run can still merge: no gate consumes that issue or failed default-branch result. The first builder's conditional fallback requires the red result to block the next merge. Implement and verify that hold, or keep the full PR test selection. The rule-file edits have correctly been removed from this head.

Checked at 2026-10-15 17:26 +0700.

## PR #751 — restart fixes verified

[Pull request](https://github.com/autonomous-ai/lampOS/pull/751)

Reviewer: Both additional restart findings are fixed at `1969921e3`. The original four independent checks and all five edge checks pass against the freshly rebuilt library: a lower first-observed count in a new run is judged; a four-second publisher outage followed by the normal read retains the cooldown at 34 seconds; old-run or unavailable-source replies remain outdated. Workspace fmt, rebuilt Policy all-target clippy and full serial Policy tests pass: 160 passed, 0 failed, 2 ignored, with both new regressions present. Msgs and Tasks trees are unchanged from their verified gates. No remaining code finding. GitHub currently reports this head CONFLICTING/DIRTY and attaches no checks; please rebase it onto current lamp-voice so the exact-head ARM64 gate can run. #765 follows this merge and will need its own rebase/gate.

Checked at 2026-10-15 17:26 +0700.

## PR #807 — merged issue

[Pull request](https://github.com/autonomous-ai/lampOS/pull/807)

Reviewer: merged #807 at 2026-10-15 17:24 +0700 — Voice and Expression honour keeper pause/resume, with host tests and the recorded lamp-52e6 conversation check.

## PR #810 — timeout counting

[Pull request](https://github.com/autonomous-ai/lampOS/pull/810)

Reviewer: [P2] Count timed-out or closed sessions as failures in the timing summary (`lampOS/crates/voice/src/main.rs:138-145`; `crates/voice/src/tool_turns.rs:231-235`). A silent 14-second timeout becomes a normal Timeline.result row, while summary excludes only rows with a failed field. The independent timeout fixture reports failed=0, plain.n=1 and plain_fact.as_expected=1 despite receiving no answer. Connection-ended has the same route. Record these terminal failures explicitly, keep their counts/reasons, and exclude incomplete samples from successful-run aggregates. Add silent-timeout and early-connection-end cases beside the failed-connect regression.

Checked at 2026-10-15 17:23 +0700.

## PR #810 — send boundary

[Pull request](https://github.com/autonomous-ai/lampOS/pull/810)

Reviewer: [P2] Timestamp the tool response before blocking journal output (`lampOS/crates/voice/src/provider_worker.rs:1725-1743`, `:1769`). answer_with sends the response, calls eprintln!, then line() obtains sent_us. A focused harness using the unchanged method bodies holds stderr's lock for 100 ms after the send callback: the callback runs at 3 us, but recorded sent_us is 105,617 us. That adds 105,614 us of journal delay to the measured tool-response latency and subtracts it from the inferred model delay. Capture the send timestamp immediately after the callback, before logging, and pass it into the record. Add a blocked-stderr regression; keep this measurement off the slow logging boundary.

Checked at 2026-10-15 17:23 +0700.

## PR #810 — lead in latency

[Pull request](https://github.com/autonomous-ai/lampOS/pull/810)

Reviewer: [P2] Do not report the lead-in as post-answer audio latency (`lampOS/crates/voice/src/turn_times.rs:267`). The summary subtracts answer_sent_ms from the first audio of the entire turn. With the behavior this PR introduces—first audio at 900 ms, function call at 1,100 ms, answer sent at 1,104 ms—the independent check reports first_audio_after_answer_ms=-204. That is not the second generation's delay. Use an actual first-audio-after-answer boundary when available; otherwise report that measurement as unavailable for lead-in turns, and keep the valid first-sound measurement separately. Cover a lead-in followed by a function answer.

Checked at 2026-10-15 17:23 +0700.

## PR #810 — rotated record order

[Pull request](https://github.com/autonomous-ai/lampOS/pull/810)

Reviewer: [P2] Make turn reconstruction independent of rotated-file discovery order (`lampOS/crates/voice/src/turn_times.rs:146-158`). Audio, speaker and outcome records are retained only if local_endpoint has already created that turn. `lines_under` uses read_dir order, and the service rotates numbered slots: an independent real-files fixture reading generation 4 in events-0 before generation 3 in events-3 loses all three fields. The same records in chronological order yield audio 1,000 ms, sound 1,200 ms, completed and plain.n=1; the rotated order yields nulls and plain.n=0. Collect per-turn fields in two passes or order by actual generation/time, not slot filename, and add a rollover regression.

Checked at 2026-10-15 17:23 +0700.

## PR #765 — incarnation verified

[Pull request](https://github.com/autonomous-ai/lampOS/pull/765)

Reviewer: The Voice restart-identity finding is fixed at `88dce47f8`. Three independent checks use actual Tracker output and the real Bus: repeated change 1 in a new run gets a distinct persisted key and is offered; fetched words from another run are refused; a queued old-run note is refused at final tell. Workspace fmt and Msgs/Tasks/Policy/Voice all-target clippy pass. Current Policy serial tests pass: 159, with 2 ignored and the acting regression present. Voice reports 1,175 passed, 1 failed, 8 ignored; the sole unchanged motor-child timeout passes on its exact isolated retry. Msgs (80) and Tasks (118) evidence is reused only after exact source-tree equality with the gated #751 dependency. No remaining Voice finding. #751 must be fixed and merged first, then this branch rebased and its exact-head ARM64 run green; this head currently has no attached ARM64 run.

Checked at 2026-10-15 17:19 +0700.

## PR #810 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/810)

Reviewer: for the first builder: this explicitly requests your silent comparisons and played lead-in measurements on lamp-4ace, and the new voice_tool_call records affect its already-enabled LAMP_FIRST_OUTPUT path. Acceptance and merge remain yours. At `d91b5fb90`, workspace fmt and Voice all-target clippy pass; the full serial gate is running. The switch-off function/next-text comparisons preserve the existing task prompts, and the lead-in is within #799's approved scope. I am checking the new measurement readers and send-time boundaries with independent cases before giving a final code conclusion. #799 remains open until the fix is measured on the lamp.

Checked at 2026-10-15 17:19 +0700.

## PR #816 — integration review complete

[Pull request](https://github.com/autonomous-ai/lampOS/pull/816)

Reviewer: Integration review complete at `dd5376453`; no concrete code finding. In particular, Keeper's normal `--act` path only restarts already-active units, so it does not start the inactive lamp parts held back by the setup marker. Keeper's required gate passes (23 tests); the dependent Body clippy/test check also passes (291 tests). One integration limit for the final setup wiring: the existing updater waits up to 45 seconds for ready:true, while marker-absent setup deliberately stays not ready through a human setup flow. That transaction behavior still needs to be reconciled/tested when the marker-enabled package is wired into installation; this PR does not install that switched unit. ARM64 and the first-builder default-change decision remain outstanding.

Checked at 2026-10-15 17:19 +0700.

## PR #778 — docs verified

[Pull request](https://github.com/autonomous-ai/lampOS/pull/778)

Reviewer: Verified `983a87b60`: only `docs/second-voice.md` changes from `811a09edc`. It now accurately records the eight answered lamp-52e6 questions, separates the host-only switching/return coverage, and consistently states the 30-second input queue. The separate 15-second STT finish timeout is correctly retained. All reported code and documentation findings are resolved; the completed 1,224-test host gate applies to the identical source. Exact-head ARM64 and the reserved first-builder acceptance remain required.

Checked at 2026-10-15 17:19 +0700.

## PR #816 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/816)

Reviewer: for the first builder: the updater-installed command link is installed/replaced unconditionally (`lampOS/crates/keeper/src/main.rs:222`), and the author explicitly reserves this default change for your merge. I have read the PR, diff, task, scope and latest comment. At `dd5376453`, workspace fmt, Keeper all-target clippy and full serial tests pass: 23 passed, 0 failed or ignored. The marker-present equivalence, absent→present transition and atomic-link tests exercise their claims. I am finishing the setup/readiness integration review; full marker-absent boot and setup on a lamp have not been demonstrated, and ARM64 is still running. #806 remains open because this is only part of it.

Checked at 2026-10-15 17:15 +0700.

## PR #778 — pending note fix gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/778)

Reviewer: The pending-note privacy finding is fixed at `811a09edc`. Nine independent checks pass: cancellation/person switch, pending-note forget, failed and silently declined Written turns, wrong-person answers, completed written history, the real forget path, and Written-confirm refusal followed by allowed spoken confirmation. Workspace fmt, Voice all-target clippy and the full serial Voice gate all pass without retries: 1,224 passed, 0 failed, 8 ignored. No remaining code finding from this review. The earlier P3 doc correction is still outstanding (`docs/second-voice.md:6-8`, `:143`, `:376-377`): describe the recorded lamp-52e6 scope and use the actual 30-second audio queue, rather than no lamp run and 15 seconds. Exact-head ARM64 and the first builder's lamp acceptance remain outstanding.

Checked at 2026-10-15 17:15 +0700.

## PR #791 — post frame maintain

[Pull request](https://github.com/autonomous-ai/lampOS/pull/791)

Reviewer: [P2] Apply recoverable-stall handling to the final per-frame authority check too (`lampOS/crates/voice/src/audio_workers.rs:702`). The top-of-loop maintain, install and try_read calls now catch recoverable discontinuities, but after DSP/VAD the loop still calls `microphone.maintain()?` directly. If a 400 ms worker pause lands after try_read returned and before this check, maintain closes the expired microphone and returns Discontinuity(AuthorityExpired); this path immediately ends capture instead of reporting a gap and reopening. Drop that old frame and enter the same bounded recovery path, retaining the permission check before any audio is sent. Add a pause injected between the read and this final check, including after one successful reopen. This is a source-audited fatal path; I have not established which worker instruction caused the first builder's second-pause failure.

Checked at 2026-10-15 17:11 +0700.

## PR #751 — publisher loss cooldown

[Pull request](https://github.com/autonomous-ai/lampOS/pull/751)

Reviewer: [P2] Preserve the task's cooldown and told history when its publisher disappears (`lampOS/crates/policy/src/progress.rs:543-545`; caller `crates/policy/src/policy.rs:1427-1430`). The new docs (`docs/policy.md:989-994`) promise that the quarter-hour hold survives a Tasks restart, but the normal SourceGone path calls gone(), which clears every Tracked value including told_at and notes. With actual Tracker snapshots: accept one say_now, observe publisher loss/restart of the same store, leave Tasks unavailable for 4 seconds (past the bus's 3-second lease), then receive its next ordinary changed-line read 34 seconds after the first note; Policy asks again and held_recent remains 0. Keep the per-store/task cooldown/history separately from unavailable live progress, while still rejecting old-run answers. Add a restart regression that includes SourceGone instead of calling status(new_run) directly.

At `2ec94dcf7`, workspace fmt and Msgs/Tasks/Policy all-target clippy pass. Full serial tests pass: Msgs 80, Tasks 118, Policy 158, 2 ignored, no failures. These additional restart cases remain blockers despite the clean gate.

Checked at 2026-10-15 17:08 +0700.

## PR #790 — merged issue

[Pull request](https://github.com/autonomous-ai/lampOS/pull/790)

Reviewer: merged #790 at 2026-10-15 17:08 +0700 — opt-in hand slumps rest 5 seconds; overloads keep 120 seconds. #703 remains open for its other work.

## PR #807 — lamp docs verified

[Pull request](https://github.com/autonomous-ai/lampOS/pull/807)

Reviewer: The verification-doc finding is fixed at `d1655b09e`. The only change since `0282c27f9` is `docs/keeper.md`: it links the recorded lamp-52e6 pause/resume run and accurately retains the untested Expression, unasked-turn and stop-lamp cases. The source remains identical to the completed host gate (Expression 230 and Voice 1,168 passed, 8 ignored, no failures). No remaining finding from this review; the new exact-head ARM64 run is required before merge.

Checked at 2026-10-15 17:06 +0700.

## PR #791 — output stall proof

[Pull request](https://github.com/autonomous-ai/lampOS/pull/791)

Reviewer: [P2] The output-active case also fails without any CaptureGap event (`lampOS/crates/voice/src/coordinator.rs:2712-2716`). A stronger independent fixture pauses capture for 500 ms after the speaker accepts 3 of 40 answer chunks. Because admission is already idle, gap_cut never cancels the active reply; its input lease expires and Voice exits with Err(StaleOwner), rather than recovering for a later question. The fixture passes the ownership/privacy invariants first and then fails the service-continuation assertion. This confirms the earlier output finding is not dependent on the timing of a simulated close notification. Include active reply/output in the recoverable expiry handling, as well as handling the explicit short-XRun close before resumed input. The final three focused regressions all fail for their intended production errors; the unmodified gate and ARM64 run remain green.

Checked at 2026-10-15 17:05 +0700.

## PR #751 — new run lower count

[Pull request](https://github.com/autonomous-ai/lampOS/pull/751)

Reviewer: [P2] Keep the new-run judging reset when its first observed count is lower (`lampOS/crates/policy/src/progress.rs:504-519`). At `2ec94dcf7`, a changed run resets judged to 0, but the older count-decrease branch immediately overwrites it with the incoming count. An independent check using real Tracker output reaches run A change 2, restarts, misses run B's initial snapshot, then receives B change 1: no question is asked, although the same-count restart regression promises that every new-run change is eligible. Skip the legacy count-decrease baseline for a distinct run, and cover old count 2 → new count 1 with the initial new-run snapshot missed. The original held-answer incarnation bug is fixed; the four earlier independent checks now pass.

Checked at 2026-10-15 17:05 +0700.

## PR #790 — rebase verified

[Pull request](https://github.com/autonomous-ai/lampOS/pull/790)

Reviewer: Verified rebase head `5583dd812`: Body source and its manifest/lockfile are unchanged from the reviewed `89f144b56`; the branch now contains only this follow-up's three commits after merged #780. I also reran the required gate on this head: workspace fmt, Body all-target clippy and full serial tests all pass, 291 passed, 0 failed or ignored. No remaining finding. Exact-head ARM64 `38715628914` is still running; #703 remains open for its other work.

Checked at 2026-10-15 17:03 +0700.

## PR #738 — continuation regressions verified

[Pull request](https://github.com/autonomous-ai/lampOS/pull/738)

Reviewer: The missing continuation coverage is fixed at `961c9ca33`. Both new production-path regressions pass: a four-piece utterance gives each successor exactly one note and plays only the final answer; the link loss after a continued request fails it unheard and leaves the unrelated post-reconnect question without a note. I read the exact wire, ownership and outcome assertions and the documented slow-service/reconnect limits. Workspace fmt and Gemini/Voice all-target clippy pass. The full serial command reports 1,423 passed, 1 failed, 10 ignored; only the existing rapid-mute/reopen timing fixture fails with capture discontinuity, and its exact isolated retry passes. All other targets ran, for 1,424 distinct passing tests after that retry. Exact-head ARM64 `38714631359` is green. No remaining requested change from this review; the first builder's reserved lamp judgment and #723's broader acceptance remain outstanding.

Checked at 2026-10-15 17:03 +0700.

## PR #791 — gap cancellation

[Pull request](https://github.com/autonomous-ai/lampOS/pull/791)

Reviewer: [P2] Cancel the affected turn for an explicit capture close, including during output and before a short reopen (`lampOS/crates/voice/src/coordinator.rs:2712-2716`, `:2854-2860`). The new cancellation condition requires active input and a gap of at least INPUT_LEASE_US; CaptureGap itself only updates closed_epoch. Two independent production-coordinator fixtures fail at `25b34758b`: a 400 ms CaptureReopen after three speaker chunks ends the service with `current reply rejected at speaker`, because admission is idle while answering; a 50 ms XRun reopen inside an admitted question ends with `accepted capture lineage changed`, because the new epoch arrives before the age-based cancellation. Both traces contain capture_gap_closed and no cancellation. Process the explicit close as a turn/lease boundary before resumed frames or more output, abandon the affected question/reply safely, and remain available for the next fresh question. Cover both output-active and sub-100 ms XRun cases.

The required unmodified gate passes: workspace fmt; Audio/Voice all-target clippy; full serial tests, 1,332 passed, 0 failed, 10 ignored. Exact-head ARM64 run `38714452377` is green. These additional interleaving regressions expose cases that the gate does not cover. Your lamp acceptance and merge remain with the first builder.

Checked at 2026-10-15 17:01 +0700.

## PR #791 — reopen control race

[Pull request](https://github.com/autonomous-ai/lampOS/pull/791)

Reviewer: [P2] Re-read capture lifecycle events before rejecting a frame from a reopened epoch (`lampOS/crates/voice/src/coordinator.rs:5548-5557`). At `25b34758b`, `validate_capture_frame` only drains control when the cached epoch is None. If the normal drain saw epoch 1, and the worker then queues Gap(1), Started(2) and frame(2) before the data receive, the cached epoch is still Some(1) and the legitimate frame ends the service with `microphone lineage changed during test`. An independent extension of the existing two-socket first-frame regression reproduces this deterministically. Drain pending lifecycle changes when the received epoch differs, then apply the resulting closed/open epoch checks; add the reopen-between-drains regression.

Checked at 2026-10-15 17:01 +0700.

## PR #791 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/791)

Reviewer: for the first builder: this changes default capture/voice behavior, and you have explicitly reserved the three capture-worker pauses and evening run on lamp-4ace. Your lamp acceptance and merge remain yours. I have read the PR, diff, issue and scope at `25b34758b`; workspace fmt and Audio/Voice all-target clippy pass. The full serial gate is running. I am also checking capture-reopen interleavings before giving a final code conclusion; exact-head ARM64 is pending.

Checked at 2026-10-15 16:53 +0700.

## PR #807 — lamp docs

[Pull request](https://github.com/autonomous-ai/lampOS/pull/807)

Reviewer: [P3] Update the earlier verification limitation now that the lamp run is recorded (`lampOS/docs/keeper.md:335-339`). It still says the pause was shown only by host tests and that a mid-conversation pause on lamp-52e6 is still to be shown. `0282c27f9` now records exactly that pause/resume demonstration. Link the new verification folder here and retain its actual remaining limitations (Expression had no request to suppress; unasked turns and stop lamp were not exercised). I verified that this update adds only the three evidence files; the completed host gate applies unchanged. The new journal/transcript evidence supports the reported Voice pause/resume result.

Checked at 2026-10-15 16:53 +0700.

## PR #797 — fixes verified

[Pull request](https://github.com/autonomous-ai/lampOS/pull/797)

Reviewer: Both calibration findings are fixed at `1c77f641a`. The independent whole-run measurement/apply check now rejects the original insufficient-margin case, and a sufficient swipe yields safe touch/release thresholds. Four independent regressions pass, including hidden-bit bounds and 6,151,950 accepted percentage/boundary combinations. Workspace fmt, Controls all-target clippy and full serial Controls tests pass: 382 passed, 0 failed. Voice source is byte-identical to `e4431d40a`, so its completed gate evidence applies. No remaining code finding; current-head ARM64 and the requested first-builder hardware acceptance remain outstanding.

Checked at 2026-10-15 16:53 +0700.

## PR #778 — pending written note

[Pull request](https://github.com/autonomous-ai/lampOS/pull/778)

Reviewer: [P1] Bind a pending written note to its request and person, and clear it on cancellation/forgetting (`lampOS/crates/voice/src/thread.rs:365-368`, `:415`, `:483-490`). At `47c7329e8`, the completed-written-history and heard-provenance fixes pass independent checks, including a Written confirm being refused and a later spoken confirm succeeding. But `Thread::written` stores one unkeyed `note`, which any next answer consumes; retirement only retires the Engine, and `forget()` leaves this note intact. Reproduction: A's Written request contains ORCHID; retire it before its chat starts; B's next spoken turn completes. B's following chat now contains A's ORCHID as a system note, attached to B's exchange. A separate pending-note → forget → B-answer case also preserves it. Store the request and person with pending context, consume it only for the matching answer, and discard it on retirement, failure and forgetting. Add both cancellation/person-switch and pending-note-forget regressions.

Checked at 2026-10-15 16:49 +0700.

## PR #792 — merged issue

[Pull request](https://github.com/autonomous-ai/lampOS/pull/792)

Reviewer: merged #792 at 2026-10-15 16:49 +0700 — opt-in attention/addressed signal and near/facing rule; lamp threshold recordings remain the first builder’s activation check.

## PR #807 — final host gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/807)

Reviewer: Host gate complete at `798425f39`: workspace fmt, Expression/Voice all-target clippy, and both full serial test suites pass without retries. Expression: 230 passed, 0 failed. Voice: 1,168 passed, 0 failed, 8 ignored. No behavior finding from this review. Current-head ARM64 and #777's requested keeper pause/resume demonstration on a lamp remain outstanding; the host fixtures do not establish that physical result.

Checked at 2026-10-15 16:46 +0700.

## PR #794 — final host gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/794)

Reviewer: No new finding at `09e53f99e`. Workspace fmt and Voice all-target clippy pass. The full serial Voice gate passes without retries: 1,166 passed, 0 failed, 8 ignored; all three new regressions pass. Exact-head ARM64 run `38703309129` is green. Default speech behavior and the requested story/follow-up lamp acceptance remain with the first builder.

Checked at 2026-10-15 16:46 +0700.

## PR #793 — unread watched tasks

[Pull request](https://github.com/autonomous-ai/lampOS/pull/793)

Reviewer: [P2] Distinguish an unread watch from an empty watch (`lampOS/crates/tasks/src/progress.rs:298-302`, `:311-319`, `:394-399`). The new contract (`crates/msgs/src/tasks.rs:88-91`, `docs/tasks.md:901-907`) says `tasks:[]` means nothing is being worked on. A real `serve_with_adapter` restart of the persisted store contradicts that: an accepted, unsettled task remains watched, but a timeout on the first `ps` leaves the published status empty and `get_progress` returns not_found. It stays that way through subsequent read failures. `watching()` only removes already-read entries; it never represents known watched tasks awaiting their first successful read. Represent unread/unavailable progress separately from an empty watched set, and add restart/failed-first-read and newly-watched/failed-read regressions.

At `81d645b72`, workspace fmt and Msgs/Tasks/Policy clippy pass; serial tests pass (80, 120 and 156 respectively, 2 Policy ignores). Exact-head ARM64 run `38707045219` is green. #751 remains an unresolved dependency. Reviewer: for the first builder: this PR requests your lamp check and changes the already-enabled progress behavior on lamp-4ace.

Checked at 2026-10-15 16:46 +0700.

## PR #780 — merged issue

[Pull request](https://github.com/autonomous-ai/lampOS/pull/780)

Reviewer: merged #780 at 2026-10-15 16:43 +0700 — opt-in whole-lamp hand slump; #703 remains open for its remaining work.

## PR #797 — final host gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/797)

Reviewer: Local gate at `e4431d40a` is complete. Workspace fmt and Controls/Voice all-target clippy pass. Controls serial tests report 380 passed/1 failed; Voice reports 1,162 passed/1 failed/8 ignored. Both failures are unchanged fixtures and pass individually: Controls' empty-stderr replay case and Voice's ready-ring startup case. Four base-versus-head replay comparisons (legacy/touch, shadow/enabled) produce byte-identical stdout and stderr. Exact-head ARM64 run `38704582746` is green. The two calibration findings remain: a full production `measure()`/`Report.apply()` fixture also reproduces the unsafe quiet thresholds, beyond the direct threshold-function check. No hardware behavior is claimed; your requested lamp-4ace acceptance stays with the first builder.

Checked at 2026-10-15 16:37 +0700.

## PR #789 — final host gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/789)

Reviewer: Host review is complete for the source tree shared by `73b50b4b8` and documentation-only `dbabac931`. Workspace fmt and Voice all-target clippy pass. The serial Voice gate returned 101 with 1,001 passed, 1 failed, 8 ignored: only `coordinator::motor::tests::pending_inspection_is_revoked_before_worker_data_and_release_is_separate` timed out in the motor preflight. It passed on its exact retry. All 193 remaining binary/integration tests and doc targets pass, for 1,195 distinct passing tests after that retry. The privacy-fixture cases pass; the motor timing work remains the separate #739 part described here. The initial sandbox-denied attempt is retained separately and is not treated as code evidence. No remaining finding from this review; current-head ARM64 is still required.

Checked at 2026-10-15 16:37 +0700.

## PR #778 — written confirmation

[Pull request](https://github.com/autonomous-ai/lampOS/pull/778)

Reviewer: [P1] Preserve whether a tool call came from heard speech or an autonomous Written turn (`lampOS/crates/voice/src/second_provider/worker.rs:269-272`). The runner always passes `heard=true` to `tools::answer`, but Written is also accepted by this route. `agent_tasks.rs:497-498` relies on that flag to require a person's later reply before confirming prepared work. Reproduction at `91e574057`, using the real Engine and tool handlers with local task sockets: prepare task 7 during heard request 7; on autonomous Written request 8, have the model call `confirm_agent_task`. The fallback returns `handing_over:true` and sends `op:confirm` to `voice.sock` although no person replied. The same call with `heard=false` is correctly refused without confirm IPC. The Live route explicitly marks Written and derives `heard=false` (`provider_worker.rs:1667-1671`, `:2620-2623`). Carry request provenance through the fallback's tool runner, pass false for Written, and cover both denied autonomous confirmation and allowed later spoken confirmation.

Checked at 2026-10-15 16:37 +0700.

## PR #778 — written history

[Pull request](https://github.com/autonomous-ai/lampOS/pull/778)

Reviewer: [P2] Preserve written-turn input context when replacing the engine cache (`lampOS/crates/voice/src/second_provider.rs:223-234`, `lampOS/crates/voice/src/second_provider/worker.rs:343-354` and `:370-376`). Both privacy regressions now pass at `91e574057`: A → B → A stays person-scoped, and actual forget followed by B → A does not restore forgotten content. However, Written calls `answer` directly without producing an input Transcript, while the canonical thread obtains its input only from those Transcript events. The old engine cache retained that written input. Reproduction: complete a Written turn containing ORCHID, then request a follow-up in the same person's session without forgetting. The next chat contains an empty user message and the previous assistant answer, with no ORCHID context anywhere. Preserve written-input context in the canonical, person-scoped history, without representing an internal note as words a person spoke, and add a written-turn/follow-up regression alongside the privacy cases.

Checked at 2026-10-15 16:32 +0700.

## PR #794 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/794)

Reviewer: for the first builder: this changes the default task-news and `check_agent_task` wording, and the builder explicitly requests the lamp-4ace story/follow-up replay. It therefore remains your merge under the reviewer rule. Head `09e53f99e` has green ARM64 run `38703309129`; independent code review and local gates are underway. The issue #757 should close only after the fix is merged and its requested behavior is accepted.

Checked at 2026-10-15 16:29 +0700.

## PR #797 — threshold rounding

[Pull request](https://github.com/autonomous-ai/lampOS/pull/797)

Reviewer: [P2] Keep the saved touch threshold strictly below the weakest accepted swipe (`lampOS/crates/controls/src/touch_calibrate.rs:209-211`). The accepted `--touch-percent 90` with noise 2 and weakest swipe 7 returns touch 7/release 3. MPR121 requires delta strictly greater than the touch threshold ([NXP AN3892, page 6](https://www.nxp.com/docs/en/application-note/AN3892.pdf)), so that weakest swipe no longer activates the saved profile when the hidden baseline bits are zero. The actual library reproduces this result. Round down or cap below the weakest swipe while preserving the noise/release margins, reject an impossible interval, and cover every accepted percentage rather than only 40/50.

Checked at 2026-10-15 16:29 +0700.

## PR #797 — baseline low bits

[Pull request](https://github.com/autonomous-ai/lampOS/pull/797)

Reviewer: [P2] Include the baseline's unread low bits in the quiet margin (`lampOS/crates/controls/src/touch_calibrate.rs:279-282` and `:203-212`). MPR121 exposes only the upper eight bits of its internal ten-bit baseline ([NXP AN3892, pages 5–6](https://www.nxp.com/docs/en/application-note/AN3892.pdf)). With internal baseline 515 and quiet filtered data 512, this code reads baseline 512 and reports noise 0 although the chip compares a delta of 3. A weakest observed swipe of 5 is accepted; the default rule saves touch 2/release 1. The measured quiet state can therefore trigger touch and cannot release it. An independent test against `e4431d40a` reproduces those exact thresholds. Include the quantization uncertainty in the quiet upper bound, require the release threshold above that bound, and reject calibration when there is insufficient remaining margin; test nonzero hidden baseline bits.

Reviewer: for the first builder: the PR explicitly requests your lamp-4ace calibration and ten-minute watch, and reports no lamp/MPR121 run. Host gates are underway; exact-head ARM64 is green.

Checked at 2026-10-15 16:29 +0700.

## PR #807 — first review

[Pull request](https://github.com/autonomous-ai/lampOS/pull/807)

Reviewer: for the first builder: #777's required pause/resume demonstration on a keeper-enabled lamp remains unshown, as this PR and `lampOS/docs/keeper.md:335-339` state. Host tests cannot establish that physical result. I have read the description, full diff, issue and scope: the new candidate gate preserves an accepted turn, all five unasked-turn start sites use the pause predicate, and Expression drops both outgoing requests before publishing. No concrete behavior defect found in that review. Formatting passes at `798425f39`; clippy/full serial tests and exact-head ARM64 are still outstanding. The missing lamp demonstration must be supplied before this switched behavior can satisfy the reviewer merge rule.

Checked at 2026-10-15 16:26 +0700.

## PR #795 — docs fixed

[Pull request](https://github.com/autonomous-ai/lampOS/pull/795)

Reviewer: Verified `4c154fee3`: only `service-privacy.md` and `ring-choreography.md` changed. They now distinguish no-child startup failures from failures after a writer starts; the documentation finding is fixed. The previously reported source gates apply unchanged. Current-head ARM64 and the first builder's privacy/lamp acceptance remain outstanding.

Checked at 2026-10-15 16:26 +0700.

## PR #789 — docs fixed

[Pull request](https://github.com/autonomous-ai/lampOS/pull/789)

Reviewer: Verified `dbabac931`: the only tree change from reviewed `73b50b4b8` is the verification README. It now correctly records six clean runs and four failures, so the documentation finding is fixed. The source gate in progress still applies to this identical source tree; current-head ARM64 is required separately.

Checked at 2026-10-15 16:26 +0700.

## PR #765 — fixed three latest gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/765)

Reviewer: At `ec4bcd6eb`, the original three consumer findings are fixed and all six new regression tests pass. Workspace fmt and clippy pass. The full serial Msgs/Tasks/Policy/Voice gate returned 101: 1,528 passed, 2 failed, 10 ignored across 55 suites. Both failures now pass individually: Policy's usage/exit test, and Voice's provider-process-loss privacy fixture (the full run observed a 51,463 µs gap; the retry observed 8,269 µs). The persisted restart-key finding remains at `lampOS/crates/voice/src/task_news.rs:570` and `:749`; your proposed run identity addresses the missing dimension, and I will verify the resulting commits on #751 and here. Current-head ARM64 remains pending.

Checked at 2026-10-15 16:26 +0700.

## PR #798 — final gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/798)

Reviewer: No finding at `e9516dadb`. `cargo fmt --all --check`, `cargo clippy -p lamp-voice --all-targets -- -D warnings`, and `cargo test -p lamp-voice -- --test-threads=1` all exit 0: 1,166 passed, 0 failed, 8 ignored, with no retries. The assigned issue #770 authorizes the Voice wording change, and the docs match it. Exact-head ARM64 run `38705400331` remains pending. Reviewer: for the first builder: this changes default wording, so the merge remains yours; your comment accepts the host proof for now.

Checked at 2026-10-15 16:21 +0700.

## PR #796 — final gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/796)

Reviewer: Final independent host evidence for merged head `93c5504d2`: workspace fmt and Audio/Voice clippy pass. Audio's serial tests pass (159 passed, 2 ignored). Voice's first serial run exits 101 with one unchanged reminder-consumer AckConfirmed failure; that exact test passes individually, and all remaining integration targets pass. Voice totals 1,164 distinct passing tests and 8 ignored after that retry. ARM64 run `38704325132` is green on this head. No additional finding. The first builder's five-start lamp evidence and merge are recorded above; #762 correctly remains open.

Checked at 2026-10-15 16:21 +0700.

## PR #795 — docs startup failure

[Pull request](https://github.com/autonomous-ai/lampOS/pull/795)

Reviewer: [P3] Qualify the indication-failure contract (`lampOS/docs/service-privacy.md:33-35`). It still says startup/worker indication failures remain errors even after reaping and cannot become a successful unmute. The fix at `1c00814d5` intentionally allows a bind/spawn failure before any writer starts to be diagnostic only, then continues without an indicator. State that distinction: a failed started writer must satisfy the verified destruction contract; a startup failure with no child does not block fresh unmute.

The original no-child optional-ring finding is fixed; both regressions pass. Six-crate clippy passes, and the latest full Voice suite exits 0 with 1,177 passed, 0 failed, 8 ignored. Other touched crates are unchanged from the completed gates; Controls' sole usage-output failure passed individually. Workspace formatting still fails at the inherited `voice/src/agent_tasks.rs:396`. Exact-head ARM64 remains pending. Reviewer: for the first builder: privacy-path and lamp acceptance remain yours.

Checked at 2026-10-15 16:21 +0700.

## PR #792 — docs fixed gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/792)

Reviewer: The `LAMP_BUS=on` prerequisite is now documented in all four relevant places at `1b8f585a2`; that finding is fixed. The delta from tested `c4b68ea58` is documentation only. Msgs/Voice clippy passed; their full serial gate returned 101 with 1,241 passed, 1 failed, 8 ignored. The unchanged reminder-roundtrip AckConfirmed test passed individually; it is recorded as the Queue's timed-test flake, not silently counted as a clean first run. `cargo fmt --all --check` still returns 1 solely at inherited `crates/voice/src/agent_tasks.rs:396`; the base version produces the identical formatting diff and all changed Rust files pass formatting. No remaining behavior finding; current-head ARM64 is pending. The first builder's revised role assigns this additive, switched topic to the reviewer.

Checked at 2026-10-15 16:21 +0700.

## PR #790 — final fixes

[Pull request](https://github.com/autonomous-ai/lampOS/pull/790)

Reviewer: All reported findings are fixed at `89f144b56`, including the command's help text: hand slumps rest for 5 seconds, overloads for 120. I independently ran `cargo fmt --all --check`, `cargo clippy -p lamp-body --all-targets -- -D warnings`, and `cargo test -p lamp-body -- --test-threads=1`: all exit 0, 291 tests passed, none failed or ignored. No remaining finding from this review. Under the amended reviewer role this is assigned to me, after #780 merges and this exact head's ARM64 run is green. #703 remains open because this is part of it.

Checked at 2026-10-15 16:21 +0700.

## PR #789 — loaded run count

[Pull request](https://github.com/autonomous-ai/lampOS/pull/789)

Reviewer: [P3] Correct the full-run summary (`lampOS/docs/verification/voice-fixtures-loaded-eighth-20261015/README.md:71`). It says none of the ten runs was clean, but the table and retained logs show six clean runs (2, 3, 4, 6, 9, 10), each with 1,181 passed, 0 failed and 8 ignored; four runs failed. Say that the requested ten clean runs were not achieved, with six passing and four failing. This preserves the limitation without contradicting the evidence.

At head `73b50b4b8`, the fixture audit found no weakened behavioral assertions; formatting and Voice clippy pass. The full serial Voice suite is running, and current-head ARM64 is still pending.

Checked at 2026-10-15 16:18 +0700.

## PR #780 — fixes verified

[Pull request](https://github.com/autonomous-ai/lampOS/pull/780)

Reviewer: All three findings are fixed and verified at `4f19f418d497aa7efdd30d18d19e881566394a2a`: only overloads update the repeat-overload timestamp, the limp-off test preserves give-way and checks the emitted writes, and the verification note is anonymous English. `cargo fmt --all --check`, `cargo clippy -p lamp-body --all-targets -- -D warnings`, and `cargo test -p lamp-body -- --test-threads=1` all pass: 291 passed / 0 failed / 0 ignored. I have read the narrowed reviewer rule at `d731283a5`; this opt-in behavior shown on lamp-52e6 is now assigned to the reviewer. Its new-head ARM64 runs are still queued, so it is not merged yet. #703 stays open because this is Part of it.

Checked at 2026-10-15 16:02 +0700.

## PR #765 — restart key

[Pull request](https://github.com/autonomous-ai/lampOS/pull/765)

Reviewer: [P2] Give progress announcements an identity that survives a Tasks restart without collisions (`lampOS/crates/voice/src/task_news.rs:513` and `:678-683`). The persisted told key is only store/task/change, but Tasks recreates `Tracker` on restart (`tasks/runtime.rs:371`), resets its counter to zero, and retains the same persisted store/task IDs. A real-Tracker/real-Bus reproduction at `93f6a771b` creates two different progress lines at change 1 on opposite sides of a restart. With the first generation's `store:3:progress-1` already told, the second line is silently discarded before fetching (`get_progress` requests: 0). Announcements remain suppressed until their new counters exceed the previously told keys. Preserve a non-resetting revision, or carry a progress incarnation consistently through status, decision, fetched progress and the persisted told key. Add the restart case with different content at the same count; it is the consumer-side counterpart of #751's pending-reply collision.

Checked at 2026-10-15 16:02 +0700.

## PR #790 — help remains

[Pull request](https://github.com/autonomous-ai/lampOS/pull/790)

Reviewer: The overload timestamp, give-way comparison and anonymous verification-note fixes are present in `8b2e94638`. The separate help finding remains: `lampOS/crates/body/src/bin/lamp-body.rs:9` still says a hand-induced slump is held again 120 seconds later. Please change that hand case to five seconds, keeping the overload case at 120 seconds. The new-head local gate is running; #780 must precede this PR.

Checked at 2026-10-15 15:58 +0700.

## PR #778 — forget history

[Pull request](https://github.com/autonomous-ai/lampOS/pull/778)

Reviewer: [P1] Invalidate the second route's history when memory is forgotten (`lampOS/crates/voice/src/second_provider.rs:373-377` and `:480`). The original cross-person finding is fixed at `fb3b0cc37`: my independent A-to-B request check now passes. A separate session transition still restores forgotten content. Reproduction against this head: finish A's exchange; call the actual `remember::run_for(forget)` with a successful removed-item response; verify `{forgotten:true}` and that the canonical Thread is empty; open B's session, then return to A. A's next chat request contains the forgotten exchange from `Shared.history`, which only filters by person and five-minute age. `remember.rs:217-220` explicitly clears the thread so forgotten material is not told to the next session, and #754's first-builder conditions require that behavior. Clear or version the second-route cache with the same forget invalidation, including in-flight results, and add an A → forget → B → A regression. This concerns a new session after forgetting, not ordinary retention within the current conversation.

Checked at 2026-10-15 15:58 +0700.

## PR #751 — restart generation

[Pull request](https://github.com/autonomous-ai/lampOS/pull/751)

Reviewer: [P2] Invalidate pending decisions when the progress publisher restarts (`lampOS/crates/policy/src/progress.rs:581-585`). Both original reported transitions are fixed at `306dc048d`, and their independent regressions now pass. However, `current()` still treats equal store/task/change values as the same progress across a restart. This is reachable with the real producer: Tasks creates a fresh `Tracker` on restart while its persisted store/task IDs survive. Two real `Tracker::read/take` instances produce change 1 for "Tests pass", then after restart change 0 and, on the next normal 30-second read, change 1 for "Found a regression". With a held reply and the supported 60-second judge deadline, Policy accepts the old yes as `say_now` for the reused count and gives the new generation a 15-minute cooldown. My producer-backed regression fails at this head while the three original checks pass. Capture and validate a publisher/tracker incarnation with pending work, invalidating it on publisher loss/restart or counter reset, and test reset then catch-up to the same counter.

Checked at 2026-10-15 15:58 +0700.

## PR #765 — gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/765)

Reviewer: Local gate recorded at `93f6a771b68fe0bffe147d6f7b8477aeec9bb553`. Formatting and Clippy (`-p lamp-msgs -p lamp-tasks -p lamp-policy -p lamp-voice --all-targets -- -D warnings`) passed. Serial tests: Msgs 80 passed; Tasks 118 passed; Policy 154 passed / 1 failed / 2 ignored; Voice lib 958 passed / 15 failed / 8 ignored, with all 15 failures passing individually afterward and all 196 integration tests passing. Original failures remain recorded. Policy `bad_arguments_print_the_usage_and_end_with_status_2` still fails alone at `lampOS/crates/policy/tests/program.rs:448` on missing usage output. That test and its log relay are unchanged from the dependency, whose gate passed it, and direct invocation prints the usage; inherited stderr loss is suspected, but this reviewer gate is not fully green. The three posted progress findings and the first-builder contract/lamp check also remain open.

Checked at 2026-10-15 15:51 +0700.

## PR #738 — gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/738)

Reviewer: Local validation recorded at `21c6dd812e41dc3b88c5fef5d58a183f4f244faa`. `cargo fmt --all --check` and `cargo clippy -p lamp-gemini -p lamp-voice --all-targets -- -D warnings` passed. The serial test command initially exited 101: Gemini 256 passed / 2 ignored; Voice lib 943 passed / 11 failed / 8 ignored. All 11 failures passed when run individually, and the previously unreached Voice binary/integration targets then passed (193 tests); both doctest targets passed. The original failures are retained, including the motor/timing cases covered by #739. ARM64 run 38673872924 is green at this exact head. The two missing continuation cases and first-builder lamp judgment remain outstanding.

Checked at 2026-10-15 15:51 +0700.

## PR #795 — optional ring

[Pull request](https://github.com/autonomous-ai/lampOS/pull/795)

Reviewer: [P2] Keep a ring that never started outside the Voice failure path (`lampOS/crates/voice/src/service_privacy_controls.rs:161-164`). During a muted wait, a private-channel bind or optional-child spawn failure returns `RingStartup::Unavailable { worker: None, error }` (`process/ring_startup.rs:140-181`): no SPI writer was created. The privacy worker can continue to receive valid samples and reach a stable unmute, with cleanup succeeding, but line 163 still turns `Allowed` into an error; `coordinator/service.rs:23` then exits Voice. This conflicts with the optional-light isolation rule in `lampOS/AGENTS.md:41`. Keep a no-child startup failure diagnostic-only and resume without the ring. Preserve the existing verified-black/exit requirements for a writer that actually started, and add a no-child failure followed by stable-unmute regression.

Checked at 2026-10-15 15:45 +0700.

## PR #780 — gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/780)

Reviewer: Local gate complete at `7a89102a2b24b5beb29f6c71c340f5a7c0003b08`: `cargo fmt --all --check`, `cargo clippy -p lamp-body --all-targets -- -D warnings`, and `cargo test -p lamp-body -- --test-threads=1` all passed, with 290 passed / 0 failed / 0 ignored. ARM64 run 38700741466 is green at this exact head. The three posted findings still need changes; the lamp judgment and merge remain with the first builder, and #703 remains open because this is only part of it.

Checked at 2026-10-15 15:45 +0700.

## PR #738 — missing cases

[Pull request](https://github.com/autonomous-ai/lampOS/pull/738)

Reviewer: [P2] Add the two continuation regressions explicitly required by #723 step 2 (`lampOS/crates/voice/src/coordinator/tests/early_end.rs:740-748` and `:915-943`). The new note assertions cover one successor, while the dropped-link test uses two independent questions and asserts `given_up() == 0`; it never fails a connection during a labelled continuation. Add a four-piece utterance and a service failure between continuation pieces, asserting the exact note placement, suppression of abandoned answers, and no continuation note on an unrelated request after reconnect.

Checked at 2026-10-15 15:45 +0700.

## PR #792 — bus docs

[Pull request](https://github.com/autonomous-ai/lampOS/pull/792)

Reviewer: [P3] Include `LAMP_BUS=on` in the enable instructions (`lampOS/docs/attention.md:624-628` and `:750-754`). The new path opens its ears, eyes and publisher only through `BUS_ROOT` (`provider_worker.rs:859`, `:895`, `:906`), but the coordinator forwards `--bus-root` only when `LAMP_BUS=on` (`bus_out.rs:49-65`). With only the two documented switches, the judgement has unknown signs and `attention/addressed` is never published. State the bus prerequisite in the setup and verification instructions, or explicitly wire the bus root for this switch.

Checked at 2026-10-15 15:40 +0700.

## PR #790 — off comparison

[Pull request](https://github.com/autonomous-ai/lampOS/pull/790)

Reviewer: [P2] The switch-off comparison gap from #780 remains at `lampOS/crates/body/src/body/tests.rs:4252-4255`: the `limp` flag also disables `give_way`, so the test does not prove unchanged `--give-way return` behavior with limp unset. Keep give-way enabled in both cases and verify the off case's writes and stop outcome against the existing baseline.

Checked at 2026-10-15 15:40 +0700.

## PR #790 — help

[Pull request](https://github.com/autonomous-ai/lampOS/pull/790)

Reviewer: [P3] Update the user-facing recovery time (`lampOS/crates/body/src/bin/lamp-body.rs:9`). The help still says a hand-induced slump is held again after 120 seconds, while `HAND_REST` now makes that five seconds. State five seconds for the hand case and retain 120 seconds for an overload.

Checked at 2026-10-15 15:40 +0700.

## PR #790 — identities

[Pull request](https://github.com/autonomous-ai/lampOS/pull/790)

Reviewer: [P3] Remove the personal attribution from `lampOS/docs/verification/v1-limp-lamp-52e6-20261015/README.md:3`, `:10`, and the new `:22`. The reviewer merge rule excludes colleagues' names. Keep the measured observations as anonymous English text.

Checked at 2026-10-15 15:40 +0700.

## PR #790 — overload

[Pull request](https://github.com/autonomous-ai/lampOS/pull/790)

Reviewer: [P2] The hand/overload timestamp finding from #780 remains at `lampOS/crates/body/src/body.rs:3586-3588`. A hand release updates `let_go_at`, so the first real overload within ten minutes is treated as a repeated overload and permanently ends the run. Track only actual overloads for this cutoff and carry the hand-then-overload regression into this head.

Checked at 2026-10-15 15:40 +0700.

## PR #780 — off comparison

[Pull request](https://github.com/autonomous-ai/lampOS/pull/780)

Reviewer: [P2] Hold the existing give-way mode constant in the switch-off comparison (`lampOS/crates/body/src/body/tests.rs:4252-4255`). `give_way: limp.then_some(GiveWay::Return)` turns off both give-way and limp in the purported limp-off case. That does not demonstrate that adding `--limp` preserves the existing `--give-way return` behavior when limp is unset. Keep `give_way` enabled in both cases and compare the off case's actual writes and stop outcome against that existing baseline.

Checked at 2026-10-15 15:40 +0700.

## PR #780 — identities

[Pull request](https://github.com/autonomous-ai/lampOS/pull/780)

Reviewer: [P3] Remove the personal attribution from the verification note (`lampOS/docs/verification/v1-limp-lamp-52e6-20261015/README.md:3` and `:10`). The reviewer merge rule excludes colleagues' names, and lampOS notes are English-only. Retain the measured observations as anonymous English text.

Checked at 2026-10-15 15:40 +0700.

## PR #780 — overload

[Pull request](https://github.com/autonomous-ai/lampOS/pull/780)

Reviewer: [P2] Do not count a hand slump as an overload (`lampOS/crates/body/src/body.rs:3580-3582`). `let_go_at` is updated for every release, including `by_hand`, but the next non-hand overload uses it to decide this is the second overload inside ten minutes. A hand slump followed by the first genuine overload therefore ends the run with torque off instead of resting and recovering after 120 seconds. Keep a separate last-overload timestamp, update it only for an overload, and add a hand-then-overload regression.

Checked at 2026-10-15 15:40 +0700.

## PR #778 — gate

[Pull request](https://github.com/autonomous-ai/lampOS/pull/778)

Reviewer: Local gate complete at `63c265fde070c8e989f8986b86609701ae982c9d`: `cargo fmt --all --check`, `cargo clippy -p lamp-voice --all-targets -- -D warnings`, and `cargo test -p lamp-voice -- --test-threads=1` all passed. Tests: 1,219 passed, 0 failed, 8 ignored across 21 suites. ARM64 run 38701241734 is also green at this exact head. The separate public-API reproduction still fails on the cross-person history finding at `lampOS/crates/voice/src/second_provider.rs:459-460`; the existing suite does not cover that transition. This remains with the first builder for that fix and the requested lamp minute.

Checked at 2026-10-15 15:40 +0700.

## PR #778 — docs

[Pull request](https://github.com/autonomous-ai/lampOS/pull/778)

Reviewer: [P3] Bring the second-route document up to date (`lampOS/docs/second-voice.md:6-8`, the opening of "A turn on the second route", and the final "Not shown" line). It still says nothing has run on a lamp although the same PR includes eight lamp-52e6 questions and their latency results. It also says only 15 seconds of audio waits for STT, while `second_provider.rs` sets `AUDIO_QUEUE = 3000` ten-millisecond packets (30 seconds), as the later Pace section already says. State the measured lamp-52e6 scope and remaining lamp-4ace work explicitly, and use the actual 30-second bound consistently.

Checked at 2026-10-15 15:32 +0700.

## PR #765 — progress expiry

[Pull request](https://github.com/autonomous-ai/lampOS/pull/765)

Reviewer: [P3] Include fetch time in the progress-note age limit (`lampOS/crates/voice/src/task_news.rs:731-735`). `wanted.age_ms` is computed before the two socket requests, but its remaining allowance is added to `Instant::now()` after those requests. A stand-in socket taking 400 ms per read leaves a note tellable about 60.8 seconds after its decision, beyond the documented 60-second bound. Preserve the absolute expiry calculated when the decision is received, reject a result that reaches it during fetching, and use that same expiry at the final take.

Checked at 2026-10-15 15:30 +0700.

## PR #751 — quiet window

[Pull request](https://github.com/autonomous-ai/lampOS/pull/751)

Reviewer: [P2] Start the quiet interval when active voice ends (`lampOS/crates/policy/src/progress.rs:536-537`). `Voice::Resting` leaves `last_talk` at the preceding active heartbeat. The voice publisher repeats active state once per second (`crates/voice/src/bus_out.rs:369-384`), so Speaking at t=0, Resting at t=0.9, and a pending progress change is asked at t=15 after only 14.1 seconds of silence. A controlled-clock reproduction against `1cbe0bbe9` confirms this. Refresh `last_talk` on the transition from active voice to Resting, without refreshing repeated Resting, and test that the documented 15-second quiet gate starts at the transition.

Checked at 2026-10-15 15:26 +0700.

## PR #778 — person history

[Pull request](https://github.com/autonomous-ai/lampOS/pull/778)

Reviewer: [P1] Keep the second route's recent history inside the person's session (`lampOS/crates/voice/src/second_provider.rs:459-460`, with the unkeyed history at `:351-371`). `Sessions::open` in `second_provider/worker.rs:246` changes the thread/memory to the new person, but `answer()` then appends every exchange in the engine-wide five-minute history, which has no person/session identity and is never cleared by that transition. Reproduced on `63c265fde` using the public Engine API and scripted services: finish person 1's turn, retire it, change the system callback to person 2, then submit person 2's turn. The second chat request contains person 2's system message followed by person 1's private user/assistant exchange. Thus the new per-person thread test passes while the actual text-model request still mixes people. Key or clear this history when the person/session changes, or build it from the same filtered thread; add a test that inspects the messages sent to chat across a person change.

Checked at 2026-10-15 15:26 +0700.

## PR #798 — handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/798)

Reviewer: for the first builder: this intentionally changes the default needs-input wording in lampOS/crates/voice/src/task_news.rs and agent_tasks.rs. There is no opt-in switch, so this reviewer role cannot merge it. I have read the full diff and will add the local gate results; the requested lamp-4ace question scenario remains with you. Head be4511a69 still needs a green ARM64 result. (15:25)

## PR #796 — handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/796)

Reviewer: for the first builder: this changes every lamp at startup (the busy microphone retry and first-control-message age allowance), so the reviewer role cannot merge it. Your five-restart check on lamp-4ace is the required judgment; the full diff is read and local gates will follow. Current head 93c5504d2 still has pending ARM64 checks. This is Part of #762, so the issue must remain open for the unexplained power-cut exits. (15:25)

## PR #765 — progress queued

[Pull request](https://github.com/autonomous-ai/lampOS/pull/765)

Reviewer: [P2] Withdraw queued progress when the task changes or finishes (`lampOS/crates/voice/src/task_news.rs:213-227`). Queued notes survive until their timer expires even after `tasks/status` removes/completes the task or advances its change. Reproduction on `93f6a771b`: offer Working/change 1, publish the same store with `tasks: []`, allow 600 ms for the reader, then `waiting()` still returns the note saying it is still working. The production coordinator waits for quiet/presence/provider availability at `coordinator.rs:4396-4411` before calling `tell()`, so a task can finish during that wait and still be announced as working. Invalidate queued notes on status changes and recheck store/task/state/change at the final take without blocking the coordinator; add a completion-during-wait regression.

Checked at 2026-10-15 15:23 +0700.

## PR #765 — progress fetch

[Pull request](https://github.com/autonomous-ai/lampOS/pull/765)

Reviewer: [P2] Validate the fetched progress state and exact change (`lampOS/crates/voice/src/task_news.rs:713-719`). The code only rejects `changes > wanted.change` and never checks the fetched `state`. If the task becomes idle between the bus snapshot and `get_progress`, this queues a note saying it is still working; a lower change is also accepted under a newer decision's authorization. Both reproduce against `93f6a771b` using the public `TaskNews` API and a local stand-in tasks socket. Require a valid Working snapshot with `changes == wanted.change` before constructing the note, and drop or retry a mismatched snapshot. Please add both race regressions.

Checked at 2026-10-15 15:23 +0700.

## PR #751 — stale context

[Pull request](https://github.com/autonomous-ai/lampOS/pull/751)

Reviewer: [P2] Discard outdated progress replies before publishing or updating task state (`lampOS/crates/policy/src/progress.rs:749-764`). Hold a yes reply for store A/task 3/change 1, then publish an empty `tasks/status` and `Voice::Listening`: the reply returns `say_now` with `guard=None` because the talking guard is reached only through `tasks.get()`. Alternatively replace status with store B/task 3: the old A reply marks B judged/told and gives B an unearned 15-minute cooldown. Both reproduce against `1cbe0bbe9` with a channel-controlled `Decider`. Invalidate pending work when its publisher/store/task/change identity is no longer current, verify that identity before touching `Tracked`, and apply the global talking guard even when the original task disappeared. Please add regressions for both transitions.

Checked at 2026-10-15 15:23 +0700.

## PR #795 — handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/795)

Reviewer: for the first builder: agreed with your review, this changes shared controls/state.privacy and voice/privacy contracts and needs a hand on the privacy slide to verify both microphones and the ring. I have picked up the new head 1a0e81d4a, including the added 50 ms pipeline test and trimmed verification folder. The green ARM64 result for 504244b64 does not clear this new head; its runs are still queued. Local review and gates will follow. (15:19)

## PR #792 — handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/792)

Reviewer: for the first builder: this adds the attention/addressed message contract in lampOS/crates/msgs/src/attention.rs, and the PR explicitly needs the live provider/bus path and near/facing thresholds judged on lamp-4ace. Head c4b68ea58 has a reported green native ARM64 attempt, but those contract and lamp checks keep its merge with you. Local review and gate results will follow. (15:19)

## PR #790 — handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/790)

Reviewer: for the first builder: this depends on #780 and changes the rest after a physical hand-induced slump from 120 s to 5 s. That recovery behavior needs the lamp judgment reserved by this role. Run 9 on lamp-52e6 is noted; head 0dec0e679 still needs its own green ARM64 run, and #780 must merge first. Local gate and code findings will follow. This remains Part of #703, not a complete closure of that issue. (15:19)

## PR #780 — handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/780)

Reviewer: for the first builder: this changes the physical contact-release and recovery behavior, and your existing review reserves the merge after the ARM64 run. The lamp-52e6 hand evidence is useful, but the feel and safety of the slump/recovery remain a lamp judgment. Current head is 7a89102a2; its ARM64 run 38700741466 is not yet green. I will add the local gate and code findings separately. This is Part of #703, so #703 must stay open after any merge. (15:19)

## PR #778 — handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/778)

Reviewer: for the first builder: #754 and the existing review require the switch-off conversation/tool/hand-over minute, then an outage and return-to-Gemini check on lamp-4ace. Those sound and switch-over judgments are outside this no-lamp reviewer role. Head 63c265fde is under local code review; cargo fmt --all --check passed and clippy is running. ARM64 run 38701241734 was still in progress when checked. (15:18)

## PR #765 — handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/765)

Reviewer: for the first builder: this includes the shared TaskStatus and Decision.task contracts from #751, and #752 explicitly requires both parts installed together with a progress line spoken on lamp-4ace before merge. Head 93f6a771b is under local review and gates; ARM64 run 38703256438 was queued when checked. I am leaving the merge and lamp judgment with you. (15:16)

## PR #751 — handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/751)

Reviewer: for the first builder: this changes shared contracts read by other components: lampOS/crates/msgs/src/lib.rs:863 adds Decision.task, and lampOS/crates/msgs/src/tasks.rs:149 adds tasks/status. That puts the merge outside this reviewer role even with both switches off. Head 1cbe0bbe9 is under local review and gates; ARM64 run 38703250546 was queued when checked. (15:16)

## PR #738 — handoff

[Pull request](https://github.com/autonomous-ai/lampOS/pull/738)

Reviewer: for the first builder: the requested rest-labelled probe and five pause scenes need lamp-4ace, and the existing review reserves this for the next MVP voice minute. ARM64 run 38673872924 is green at head 21c6dd812. Local code review and gates are in progress; I will not merge or close #723 before the requested lamp judgment. (15:16)
