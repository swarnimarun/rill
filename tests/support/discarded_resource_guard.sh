#!/bin/bash
set -eu
: "${K4_PERSISTENT_ROOT:?Set K4_PERSISTENT_ROOT to the persistent filesystem root}"
: "${K4_RAM_ROOT:?Set K4_RAM_ROOT to the RAM filesystem containing source and target}"
persistent=$(df -Pk "$K4_PERSISTENT_ROOT" | awk 'NR==2{print $4}')
ram=$(df -Pk "$K4_RAM_ROOT" | awk 'NR==2{print $4}')
mem=$(awk '/MemAvailable:/{print $2}' /proc/meminfo)
printf 'persistent_kib=%s ram_kib=%s mem_available_kib=%s\n' "$persistent" "$ram" "$mem"
test "$persistent" -ge 2097152 && test "$ram" -ge 2097152 && test "$mem" -ge 3145728
