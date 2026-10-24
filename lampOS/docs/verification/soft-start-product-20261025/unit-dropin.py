# unit-dropin.py UNIT RUNNING_ENV OUT_CONF OUT_ENV (25 Oct 04:5x, first builder): a drop-in that sets lamp-4ace's voice
# exactly as a product unit sets it, for a round on that exact unit. Every LAMP_* the unit sets is set as written there;
# every other LAMP_* the running voice has is unset. The device's own settings are kept, and the product paths 4ace
# lacks are left out (the same six exceptions as zz-product-f.conf). OUT_ENV is the unit's LAMP_* as the voice should
# then read them, for the round's switch-by-switch check.
import shlex, sys
unit, running, out_conf, out_env = sys.argv[1:5]
DEVICE = {'LAMP_SAY_GROUP', 'LAMP_BUS_ROOT', 'LAMP_ONBOARDING_FILE'}
ABSENT = {'LAMP_PRIVACY_CONTROLS', 'LAMP_PERSON_NAME', 'LAMP_TASKS_NEEDS'}
lines, env = [], {}
for raw in open(unit):
    raw = raw.strip()
    if not raw.startswith('Environment='):
        continue
    for item in shlex.split(raw[len('Environment='):]):
        k, _, v = item.partition('=')
        if k.startswith('LAMP_') and k not in DEVICE | ABSENT:
            env[k] = v
            lines.append('Environment=' + shlex.quote(item) if ' ' in item else 'Environment=' + item)
have = {l.split('=', 1)[0] for l in open(running) if l.startswith('LAMP_')}
unset = sorted(k for k in have if k not in env and k not in DEVICE)
with open(out_conf, 'w') as f:
    f.write(f'# Made by unit-dropin.py from {unit}: the voice as that unit sets it; device settings kept, product-only paths left out.\n')
    f.write('[Service]\n')
    if unset:
        f.write('UnsetEnvironment=' + ' '.join(unset) + '\n')
    f.write('\n'.join(lines) + '\n')
with open(out_env, 'w') as f:
    f.write(''.join(f'{k}={v}\n' for k, v in sorted(env.items())))
print(f'{len(env)} set, {len(unset)} unset')
