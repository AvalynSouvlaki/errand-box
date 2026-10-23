#!/bin/sh
# One workload, named by $1, run in a fresh directory under $WL_DIR (default
# /tmp) of whatever sandbox this runs in. Prints its own elapsed milliseconds, and only when the
# workload succeeded. Needs sh, python3, git, gcc and make.
#   noop    nothing: the floor
#   spawn   300 fork+exec of /bin/true
#   files   write, read back and stat 3000 files of 512 bytes
#   git     init, 1000 files, add, commit, verify
#   cpu     a 3e6-iteration python loop
#   compile 200 generated C files, make -j4, link, run
set -u
# WL_DIR names where the work happens. The engines differ in where their own
# /tmp lives (gVisor keeps the root filesystem in memory; runc writes to the
# host disk), so the caller chooses the same kind of directory for every one.
d="${WL_DIR:-/tmp}/wl.$$"; rm -rf "$d"; mkdir -p "$d"; cd "$d" || exit 1
t0=$(date +%s%N)
(
set -e
case "$1" in
  noop) : ;;
  spawn) i=0; while [ $i -lt 300 ]; do /bin/true; i=$((i+1)); done ;;
  files) python3 -I -c "
import os
for i in range(3000):
    open(f'f{i}', 'w').write('x' * 512)
n = sum(len(open(f'f{i}').read()) for i in range(3000))
for i in range(3000):
    os.stat(f'f{i}')
assert n == 3000 * 512
" ;;
  git) git init -q .
       i=0; while [ $i -lt 1000 ]; do echo $i > f$i; i=$((i+1)); done
       git -c user.name=t -c user.email=t@t add -A
       git -c user.name=t -c user.email=t@t commit -qm x
       test "$(git rev-list --count HEAD)" = 1 ;;
  cpu) python3 -I -c "
s = 0
for i in range(3_000_000):
    s += i * i
" ;;
  compile)
       i=0; while [ $i -lt 200 ]; do
         printf 'int f%d(int x){int s=0;for(int i=0;i<x;i++)s+=i*%d;return s;}\n' $i $i > f$i.c
         i=$((i+1)); done
       { printf 'int f0(int);\nint main(void){return f0(3)==3*0?0:1;}\n'; } > main.c
       printf 'SRC=$(wildcard *.c)\nOBJ=$(SRC:.c=.o)\nall: prog\nprog: $(OBJ)\n\tgcc -o $@ $^\n%%.o: %%.c\n\tgcc -O2 -c $< -o $@\n' > Makefile
       make -s -j4
       ./prog ;;
  *) echo "unknown workload $1" >&2; exit 2 ;;
esac
) > "$d.err" 2>&1
code=$?
t1=$(date +%s%N)
cd / && rm -rf "$d"
if [ $code -eq 0 ]; then echo "inner_ms=$(( (t1 - t0) / 1000000 ))"; else echo "workload $1 failed: $(head -3 "$d.err")" >&2; fi
rm -f "$d.err"
exit $code
