# lamp, root sh (row common first). READ ONLY. The optional put-back step (run.sh putback, PUTBACK_CMD): lamp-update's
# installed sequence and whether a round is under way. Prints SEQ installed=<N> busy='<what>'.
set -u
echo "SEQ installed=$(build) busy='$(updbusy)'"
