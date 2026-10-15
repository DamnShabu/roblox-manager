//! How much of the machine each running client takes -- its CPU, its memory
//! and how long it has run -- and the machine's whole load. One `ps` lists
//! this user's processes (each one's parent, age and resident memory); one
//! `cat` reads the CPU time of every process a client is made of, and the
//! machine's totals, from /proc. Both run where the clients are seen, so a
//! Flatpak reads the host's. CPU is a share of the whole machine since the
//! reading before: the first reading has memory and age, and no CPU.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use super::process::last_line;
use super::{CordialError, CordialProfiles};
use crate::types::Profile;

/// What one client takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClientUsage {
    /// Its share of the whole machine's CPU since the last reading, 0-100;
    /// None on the first reading of it.
    pub cpu: Option<f32>,
    /// Resident memory of its engine and everything that started, in bytes.
    pub memory: u64,
    /// How long its engine has run.
    pub uptime: Duration,
}

/// The machine as a whole.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MachineLoad {
    /// How busy all its CPUs were since the last reading, 0-100; None on
    /// the first.
    pub cpu: Option<f32>,
    /// In bytes: what is in use (what is not available) and the total.
    pub memory_used: u64,
    pub memory_total: u64,
}

/// One reading: each client by its profile, and the machine.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reading {
    pub clients: HashMap<Profile, ClientUsage>,
    /// None when /proc did not say.
    pub machine: Option<MachineLoad>,
}

/// What a reading keeps for the next: the CPU ticks spent so far by the
/// machine and by each client, by its engine's pid.
#[derive(Debug, Default)]
pub struct Meter {
    machine: Option<Ticks>,
    clients: HashMap<u32, u64>,
}

/// CPU ticks: busy, and all of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Ticks {
    busy: u64,
    total: u64,
}

/// What `cat` read: the machine's CPU ticks, its memory (total, available)
/// in bytes, and the CPU ticks each listed process has spent.
struct Cat {
    machine: Option<Ticks>,
    memory: Option<(u64, u64)>,
    spent: HashMap<u32, u64>,
}

/// One process as `ps` lists it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Listed {
    pid: u32,
    ppid: u32,
    /// Seconds since it started.
    age: u64,
    rss_kib: u64,
}

/// What `ps` is asked for, in this order.
const PS_FIELDS: &str = "pid=,ppid=,etimes=,rss=";

impl CordialProfiles {
    /// What each of `clients` -- {engine pid: profile}, as
    /// [`CordialProfiles::clients`] found them -- takes now, and the
    /// machine's load. `meter` carries the last reading, for the CPU.
    pub fn usage(
        &self,
        meter: &mut Meter,
        clients: &BTreeMap<u32, Profile>,
    ) -> Result<Reading, CordialError> {
        let uid = rustix::process::getuid().as_raw().to_string();
        let ps = self.run_seen(&["ps", "-u", &uid, "-o", PS_FIELDS])?;
        let ps = String::from_utf8_lossy(&ps.stdout).into_owned();
        let listed = parse_ps(&ps);
        let stats: Vec<String> = clients
            .keys()
            .flat_map(|root| tree(*root, &listed))
            .map(|pid| format!("/proc/{pid}/stat"))
            .collect();
        let mut files = vec!["/proc/stat", "/proc/meminfo"];
        files.extend(stats.iter().map(String::as_str));
        let mut argv = vec!["cat"];
        argv.extend(files);
        // A process that ended since `ps` makes cat fail on its file and
        // print the rest: what it printed is the reading.
        let cat = self.run_seen(&argv)?;
        if cat.stdout.is_empty() {
            return Err(CordialError::Process(format!(
                "could not read the clients' CPU use: {}",
                last_line(&cat.stderr)
            )));
        }
        Ok(meter.update(clients, &ps, &String::from_utf8_lossy(&cat.stdout)))
    }
}

impl Meter {
    /// A reading from what `ps` (with [`PS_FIELDS`]) and `cat` (of
    /// /proc/stat, /proc/meminfo and each process's stat) printed.
    pub fn update(&mut self, clients: &BTreeMap<u32, Profile>, ps: &str, cat: &str) -> Reading {
        let listed = parse_ps(ps);
        let Cat { machine: machine_ticks, memory, spent } = parse_cat(cat);
        let elapsed = match (self.machine, machine_ticks) {
            (Some(was), Some(now)) if now.total > was.total => {
                Some((now.busy.saturating_sub(was.busy), now.total - was.total))
            }
            _ => None,
        };
        let share = |used: u64, total: u64| (used as f64 * 100.0 / total as f64).min(100.0) as f32;
        let mut ticks_now = HashMap::new();
        let mut out = HashMap::new();
        for (root, profile) in clients {
            let Some(engine) = listed.iter().find(|p| p.pid == *root) else { continue };
            let pids = tree(*root, &listed);
            let memory = listed
                .iter()
                .filter(|p| pids.contains(&p.pid))
                .map(|p| p.rss_kib.saturating_mul(1024))
                .sum();
            let ticks: u64 = pids.iter().filter_map(|pid| spent.get(pid)).sum();
            let cpu = match (self.clients.get(root), elapsed) {
                (Some(was), Some((_, total))) => Some(share(ticks.saturating_sub(*was), total)),
                _ => None,
            };
            ticks_now.insert(*root, ticks);
            out.insert(
                profile.clone(),
                ClientUsage { cpu, memory, uptime: Duration::from_secs(engine.age) },
            );
        }
        let machine = memory.map(|(total, available)| MachineLoad {
            cpu: elapsed.map(|(busy, total)| share(busy, total)),
            memory_used: total.saturating_sub(available),
            memory_total: total,
        });
        self.machine = machine_ticks;
        self.clients = ticks_now;
        Reading { clients: out, machine }
    }
}

/// `ps -o pid=,ppid=,etimes=,rss=`: one process a line; a line that does
/// not read is skipped.
fn parse_ps(ps: &str) -> Vec<Listed> {
    ps.lines()
        .filter_map(|line| {
            let mut f = line.split_whitespace().map(str::parse::<u64>);
            let mut next = || f.next()?.ok();
            let (pid, ppid, age, rss_kib) = (next()?, next()?, next()?, next()?);
            Some(Listed {
                pid: u32::try_from(pid).ok()?,
                ppid: u32::try_from(ppid).ok()?,
                age,
                rss_kib,
            })
        })
        .collect()
}

/// `root` and every process under it.
fn tree(root: u32, listed: &[Listed]) -> Vec<u32> {
    let mut found = vec![root];
    let mut seen: HashSet<u32> = found.iter().copied().collect();
    let mut i = 0;
    while let Some(&pid) = found.get(i) {
        for child in listed.iter().filter(|p| p.ppid == pid) {
            if seen.insert(child.pid) {
                found.push(child.pid);
            }
        }
        i += 1;
    }
    found
}

/// From `cat /proc/stat /proc/meminfo /proc/<pid>/stat...`: the machine's
/// CPU ticks, its memory (total, available) in bytes, and the CPU ticks
/// each listed process has spent.
fn parse_cat(cat: &str) -> Cat {
    let mut machine = None;
    let (mut total, mut available) = (None, None);
    let mut spent = HashMap::new();
    for line in cat.lines() {
        if let Some(rest) = line.strip_prefix("cpu ") {
            machine = machine_ticks(rest);
        } else if let Some(kib) = line.strip_prefix("MemTotal:") {
            total = kib_to_bytes(kib);
        } else if let Some(kib) = line.strip_prefix("MemAvailable:") {
            available = kib_to_bytes(kib);
        } else if let Some((pid, ticks)) = process_ticks(line) {
            spent.insert(pid, ticks);
        }
    }
    Cat { machine, memory: total.zip(available), spent }
}

/// /proc/stat's `cpu` line after its name: user nice system idle iowait irq
/// softirq steal (guest time is already counted in user).
fn machine_ticks(rest: &str) -> Option<Ticks> {
    let f: Vec<u64> =
        rest.split_whitespace().take(8).map(str::parse).collect::<Result<_, _>>().ok()?;
    let [user, nice, system, idle, iowait, irq, softirq, steal] = f.try_into().ok()?;
    let total = user + nice + system + idle + iowait + irq + softirq + steal;
    Some(Ticks { busy: total - idle - iowait, total })
}

/// "  32768000 kB" as bytes.
fn kib_to_bytes(s: &str) -> Option<u64> {
    let kib: u64 = s.trim().strip_suffix("kB")?.trim().parse().ok()?;
    Some(kib.saturating_mul(1024))
}

/// A /proc/<pid>/stat line: its pid, and the ticks it has spent (utime +
/// stime). Its name, in brackets, may hold spaces and brackets of its own,
/// so the fields are counted from the last ") ".
fn process_ticks(line: &str) -> Option<(u32, u64)> {
    let (head, tail) = line.rsplit_once(") ")?;
    let pid: u32 = head.split_once(" (")?.0.trim().parse().ok()?;
    // After the name: state(3) ppid(4) ... utime(14) stime(15).
    let mut f = tail.split_whitespace().skip(11);
    let utime: u64 = f.next()?.parse().ok()?;
    let stime: u64 = f.next()?.parse().ok()?;
    Some((pid, utime + stime))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PS: &str = "    1     0  9999  12000\n  100     1   600   4000\n  101   100   590 500000\n  102   101   580  20000\n  200     1   120 300000\n  300     1    40   1000\n";

    fn stat_line(pid: u32, name: &str, utime: u64, stime: u64) -> String {
        format!(
            "{pid} ({name}) S 1 1 1 0 -1 4194560 100 0 0 0 {utime} {stime} 0 0 20 0 1 0 1 1 1\n"
        )
    }

    fn cat(busy_user: u64, idle: u64, procs: &[(u32, &str, u64)]) -> String {
        let mut s = format!(
            "cpu  {busy_user} 0 0 {idle} 0 0 0 0 0 0\ncpu0 1 0 0 1 0 0 0 0 0 0\nintr 1 2\nctxt 5\n\
             MemTotal:       32000000 kB\nMemFree:         1000000 kB\nMemAvailable:   20000000 kB\n"
        );
        for (pid, name, ticks) in procs {
            s.push_str(&stat_line(*pid, name, *ticks, 0));
        }
        s
    }

    fn clients() -> BTreeMap<u32, Profile> {
        BTreeMap::from([(100, Profile::named("rbxmgr-1")), (200, Profile::named("rbxmgr-2"))])
    }

    #[test]
    fn a_client_is_its_engine_and_everything_under_it() {
        let listed = parse_ps(PS);
        assert_eq!(tree(100, &listed), [100, 101, 102]);
        assert_eq!(tree(300, &listed), [300]);
    }

    #[test]
    fn the_first_reading_has_memory_and_age_but_no_cpu() {
        let mut m = Meter::default();
        let r = m.update(
            &clients(),
            PS,
            &cat(
                1000,
                9000,
                &[(100, "cordial-run", 50), (101, "x", 50), (102, "y", 0), (200, "z", 10)],
            ),
        );
        let one = r.clients[&Profile::named("rbxmgr-1")];
        assert_eq!(one.memory, (4000 + 500_000 + 20000) * 1024);
        assert_eq!(one.uptime, Duration::from_secs(600));
        assert_eq!(one.cpu, None);
        let machine = r.machine.unwrap();
        assert_eq!(
            (machine.memory_total, machine.memory_used),
            (32_000_000 * 1024, 12_000_000 * 1024)
        );
        assert_eq!(machine.cpu, None);
    }

    #[test]
    fn cpu_is_each_clients_share_of_the_machine_since_the_last_reading() {
        let mut m = Meter::default();
        m.update(
            &clients(),
            PS,
            &cat(1000, 9000, &[(100, "e", 50), (101, "e", 50), (200, "e", 10)]),
        );
        // 1000 ticks pass on the machine, 400 of them busy; client 1 spends 150, client 2 50.
        let r = m.update(
            &clients(),
            PS,
            &cat(1400, 9600, &[(100, "e", 120), (101, "e", 130), (200, "e", 60)]),
        );
        let cpu = |p: &str| r.clients[&Profile::named(p)].cpu.unwrap();
        assert!((cpu("rbxmgr-1") - 15.0).abs() < 0.01, "{}", cpu("rbxmgr-1"));
        assert!((cpu("rbxmgr-2") - 5.0).abs() < 0.01);
        assert!((r.machine.unwrap().cpu.unwrap() - 40.0).abs() < 0.01);
    }

    #[test]
    fn a_client_new_since_the_last_reading_has_no_cpu_yet() {
        let mut m = Meter::default();
        let only_one = BTreeMap::from([(100, Profile::named("rbxmgr-1"))]);
        m.update(&only_one, PS, &cat(1000, 9000, &[(100, "e", 50)]));
        let r = m.update(&clients(), PS, &cat(1400, 9600, &[(100, "e", 60), (200, "e", 60)]));
        assert!(r.clients[&Profile::named("rbxmgr-1")].cpu.is_some());
        assert_eq!(r.clients[&Profile::named("rbxmgr-2")].cpu, None);
    }

    #[test]
    fn a_client_ps_did_not_list_is_left_out() {
        let mut m = Meter::default();
        let gone = BTreeMap::from([(999, Profile::named("rbxmgr-9"))]);
        assert!(m.update(&gone, PS, &cat(1, 1, &[])).clients.is_empty());
    }

    #[test]
    fn a_process_name_with_spaces_and_brackets_still_reads() {
        assert_eq!(process_ticks(&stat_line(42, "Web Content (2)) x", 7, 3)), Some((42, 10)));
        assert_eq!(process_ticks("cpu  1 2 3"), None);
        assert_eq!(process_ticks("MemTotal: 5 kB"), None);
    }

    #[test]
    fn a_reading_runs_ps_then_cat_where_the_clients_are_seen() {
        use std::sync::Arc;

        use crate::cordial::ProcessView;
        use crate::cordial::process::recording::Recording;
        use crate::keyring::{Keyring, MemorySecrets};
        use crate::paths::Paths;

        let dir = tempfile::tempdir().unwrap();
        // 102 ended between the two: cat says so, and prints the rest.
        let runner = Arc::new(Recording::default().answer(0, PS, "").answer(
            1,
            &cat(1, 1, &[(100, "e", 5), (101, "e", 5)]),
            "cat: /proc/102/stat: No such file or directory",
        ));
        let keyring = Arc::new(Keyring::new(Box::new(Arc::new(MemorySecrets::default()))));
        let profiles = CordialProfiles::new(
            keyring,
            &Paths::under(dir.path()),
            runner.clone(),
            ProcessView::Host,
            Arc::new(|_| {}),
        );
        let r = profiles.usage(&mut Meter::default(), &clients()).unwrap();
        assert_eq!(r.clients.len(), 2);
        let ran = runner.ran();
        assert_eq!(ran[0][..3], ["flatpak-spawn", "--host", "ps"]);
        assert_eq!(ran[1][..5], ["flatpak-spawn", "--host", "cat", "/proc/stat", "/proc/meminfo"]);
        for pid in [100, 101, 102, 200] {
            assert!(ran[1].contains(&format!("/proc/{pid}/stat")), "{pid}");
        }
        assert!(!ran[1].contains(&"/proc/300/stat".to_owned()), "not a client's");
    }

    #[test]
    fn what_does_not_read_is_skipped_not_fatal() {
        assert!(parse_ps("PID PPID\n x y z w\n").is_empty());
        let Cat { machine, memory, spent } = parse_cat("garbage\ncpu  a b c\n");
        assert_eq!((machine, memory, spent.len()), (None, None, 0));
    }
}
