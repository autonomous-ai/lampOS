#!/bin/bash
# gh stand-in for the DRY matrix: records the call instead of posting.
printf '%s %s\n' "$(date +%H:%M:%S)" "$*" >>"${GH_STUB_OUT:?}"
