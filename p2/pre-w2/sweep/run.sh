#!/usr/bin/env bash
# P2 pre-W2 — containment primitives: scope limits, cgroup.kill (kill path),
# PDEATHSIG parent-death, fork-inheritance check, stale-scope startup sweep.
# Privileged container + keeper prep (see p2/cgroup-matrix recipe).
set -u
echo "===== A. keeper prep ====="
mkdir /sys/fs/cgroup/keeper 2>/dev/null
for p in $(cat /sys/fs/cgroup/cgroup.procs); do echo $p > /sys/fs/cgroup/keeper/cgroup.procs 2>/dev/null; done
echo '+cpu +memory +pids' > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null
echo "nsroot subtree_control: [$(cat /sys/fs/cgroup/cgroup.subtree_control)]"
echo "nsroot procs after prep: [$(cat /sys/fs/cgroup/cgroup.procs)]"

echo "===== B. session scope + limits ====="
mkdir /sys/fs/cgroup/dai-sess1
echo 100000000 > /sys/fs/cgroup/dai-sess1/memory.max
echo 64 > /sys/fs/cgroup/dai-sess1/pids.max
sleep 600 & SPAWNER=$!
sleep 600 & CL=$!
sleep 1
echo $SPAWNER > /sys/fs/cgroup/dai-sess1/cgroup.procs
echo $CL > /sys/fs/cgroup/dai-sess1/cgroup.procs
echo "scope procs: [$(cat /sys/fs/cgroup/dai-sess1/cgroup.procs)]"
echo "readbacks: memory.max=$(cat /sys/fs/cgroup/dai-sess1/memory.max) pids.max=$(cat /sys/fs/cgroup/dai-sess1/pids.max)"

echo "===== B2. fork inheritance: children born AFTER parent is in the scope land in the scope ====="
mkdir /sys/fs/cgroup/dai-inh
python3 -c "
import os, time
os.system('echo %d > /sys/fs/cgroup/dai-inh/cgroup.procs' % os.getpid())
pid = os.fork()
if pid == 0:
    open('/tmp/inh.txt','w').write(open('/proc/self/cgroup').read().strip())
    time.sleep(600); os._exit(0)
time.sleep(600)
" &
IHPID=$!
sleep 2
echo "forked-child cgroup: [$(cat /tmp/inh.txt 2>/dev/null)]"
echo "scope procs (parent+child): [$(cat /sys/fs/cgroup/dai-inh/cgroup.procs)]"
echo 1 > /sys/fs/cgroup/dai-inh/cgroup.kill
sleep 1
echo "post-kill: [$(cat /sys/fs/cgroup/dai-inh/cgroup.procs)]"
rmdir /sys/fs/cgroup/dai-inh && echo "inherit scope removed" || echo "inherit scope rmdir FAIL"

echo "===== C. kill path: cgroup.kill ====="
echo 1 > /sys/fs/cgroup/dai-sess1/cgroup.kill
sleep 1
echo "procs after kill: [$(cat /sys/fs/cgroup/dai-sess1/cgroup.procs)]"
echo "spawner alive=$(kill -0 $SPAWNER 2>/dev/null && echo yes || echo no); client alive=$(kill -0 $CL 2>/dev/null && echo yes || echo no)"
rmdir /sys/fs/cgroup/dai-sess1 && echo "scope rmdir OK" || echo "scope rmdir FAIL"

echo "===== D. PDEATHSIG: child dies when parent is SIGKILLed ====="
rm -f /tmp/pdeath.pid
python3 -c "
import os, signal, time, ctypes
libc = ctypes.CDLL(None)
pid = os.fork()
if pid == 0:
    libc.prctl(1, signal.SIGKILL)   # PR_SET_PDEATHSIG
    open('/tmp/pdeath.pid','w').write(str(os.getpid()))
    time.sleep(600); os._exit(0)
time.sleep(600)
" &
MPID=$!
sleep 2
CPID=$(cat /tmp/pdeath.pid 2>/dev/null)
echo "manager=$MPID child=$CPID"
kill -9 $MPID
sleep 2
echo "child alive after manager SIGKILL: $(kill -0 $CPID 2>/dev/null && echo YES || echo no)"

echo "===== E. stale-scope startup sweep (manager-crash simulation) ====="
mkdir /sys/fs/cgroup/dai-sess2
sleep 600 & SP2=$!
sleep 1
echo $SP2 > /sys/fs/cgroup/dai-sess2/cgroup.procs
echo "pre-sweep procs in dai-sess2: [$(cat /sys/fs/cgroup/dai-sess2/cgroup.procs)]"
echo "-- sweep pass (what the manager runs at startup):"
for s in /sys/fs/cgroup/dai-sess*; do
  echo "  sweeping $s"
  echo 1 > $s/cgroup.kill 2>/dev/null
  sleep 1
  rmdir $s 2>/dev/null && echo "    removed" || echo "    rmdir FAIL"
done
ls -d /sys/fs/cgroup/dai-sess* 2>/dev/null || echo "no stale scopes remain"
echo "process alive check: $(kill -0 $SP2 2>/dev/null && echo YES || echo no)"
echo "=== done ==="
