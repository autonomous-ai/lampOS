# BODY gw
# lamp, root sh (common first). Leg G's cold start: lamp-openclaw-gateway restarted (1 start; never stopped for good,
# lamp-cloud untouched). Prints GW RESTARTED <epoch>.
systemctl restart lamp-openclaw-gateway && { n "restarted lamp-openclaw-gateway (leg G cold start)"; echo "GW RESTARTED $(date +%s)"; } || echo "GW NOT RESTARTED"
