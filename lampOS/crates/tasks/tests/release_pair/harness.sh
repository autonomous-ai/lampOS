#!/bin/sh
# Builtins only: one known local process per invocation, never a real Harness.
set -eu
umask 077
fail() {
    printf '%s\n' 'unexpected fixture command' >> "$HOME/unexpected"
    exit 97
}
{
    printf '%s' "$1"
    for arg in "$@"; do printf '\t%s' "$arg"; done
    printf '\n'
} >> "$HOME/calls.tsv"
IFS= read -r engine < "$HOME/engine"
machine=0123456789abcdef0123456789abcdef
case "$1" in
    machines)
        [ "$#" = 2 ] && [ "$2" = --json ] || fail
        printf '{"machineId":"%s","current":true}\n' "$machine"
        ;;
    auth)
        [ "$#" = 3 ] && [ "$2" = status ] && [ "$3" = --json ] || fail
        printf '%s\n' '{"loggedIn":true}'
        ;;
    new)
        [ "$2" = "$engine" ] && [ "$3" = --machine ] && [ "$4" = "$machine" ] || fail
        shift 4
        if [ "$engine" != devin-cloud ]; then
            [ "$1" = /fixture/project ] || fail
            shift
        fi
        [ "$1" = --name ] || fail
        name=$2
        suffix=${name#lamp-}
        [ "$suffix" != "$name" ] && [ "${#suffix}" = 32 ] || fail
        case "$suffix" in *[!0123456789abcdef]*) fail;; esac
        shift 2
        mode=null
        bypass=false
        if [ "$engine" = claude ]; then
            [ "$1" = --mode ] && [ "$2" = auto ] || fail
            mode='"auto"'
            bypass=true
            shift 2
        fi
        [ "$#" = 3 ] && [ "$1" = --json ] && [ "$2" = --task ] || fail
        case "$3" in
            'Pair working') digit=1;;
            'Pair answered') digit=2;;
            'Pair blocked')
                printf '%s\n' "$$" > "$HOME/blocked.pid"
                # The parent kills/reaps this owned process after killing Tasks.
                # A successful read would mean an unexpected release of the cut.
                IFS= read -r released < "$HOME/release.fifo"
                fail
                ;;
            *) fail;;
        esac
        if [ "$engine" = devin-cloud ]; then
            id="cloud_${digit}${digit}${digit}${digit}${digit}${digit}${digit}${digit}${digit}${digit}${digit}${digit}${digit}${digit}${digit}${digit}"
        else
            id="${digit}${digit}${digit}${digit}${digit}${digit}${digit}${digit}-${digit}${digit}${digit}${digit}-4${digit}${digit}${digit}-8${digit}${digit}${digit}-${digit}${digit}${digit}${digit}${digit}${digit}${digit}${digit}${digit}${digit}${digit}${digit}"
        fi
        printf '{"ok":true,"machineId":"%s","agent":{"id":"%s","name":"%s","engine":"%s","permissionMode":%s,"bypassPermission":%s}}\n' \
            "$machine" "$id" "$name" "$engine" "$mode" "$bypass"
        ;;
    show)
        [ "$#" = 7 ] && [ "$2" = --machine ] && [ "$3" = "$machine" ] && \
            [ "$5" = --chars ] && [ "$6" = 4000 ] && [ "$7" = --json ] || fail
        case "$4" in
            11111111-1111-4111-8111-111111111111|cloud_1111111111111111)
                state=working; ask='Pair working'; answer='Fixture still working';;
            22222222-2222-4222-8222-222222222222|cloud_2222222222222222)
                state=idle; ask='Pair answered'; answer='Fixture answer retained';;
            *) fail;;
        esac
        case "$engine:$4" in
            devin-cloud:cloud_*|claude:????????-????-????-????-????????????|devin:????????-????-????-????-????????????) ;;
            *) fail;;
        esac
        printf '{"id":"%s","state":"%s","ask":"%s","answer":"%s","truncated":false,"tools":2}\n' \
            "$4" "$state" "$ask" "$answer"
        ;;
    *) fail;;
esac
