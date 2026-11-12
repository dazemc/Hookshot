// Linux Play from a desktop launcher: start the game in a systemd scope of
// its own, then close the builder only after the hand-off is real.
//
// A desktop that starts each launcher as a service of its own
// (app-hookshot@<id>.service: COSMIC, GNOME, KDE, SteamOS) stops that
// service, and every process left in its control group, when the launcher
// exits. `Process.start(..., detached)` leaves the session, not the control
// group, so Play closing the window had systemd kill the game too, about
// 90 seconds later (#2529). `systemd-run --user --scope` moves the game to a
// scope that outlives the service.
//
// A trial `systemd-run ... true` runs first. The real launch then waits until
// the runner's cgroup differs from the launcher's before letting it exec
// (upstream #2540 and #2643). If that proof does not come within two seconds,
// the gate is closed without permission and the game starts directly once,
// exactly as it did before.
import 'dart:async';
import 'dart:io';

/// The scope arguments after `systemd-run`, before the game's command.
const scopeArguments = ['--user', '--scope', '--collect', '--quiet', '--'];

/// A scoped launch waiting behind the hand-off gate.
abstract class ScopedProcess {
  int get pid;
  bool get exited;
  Future<int> get exitCode;

  /// Let the command exec: the hand-off was proven.
  Future<void> allow();

  /// Close the gate without permission: a late scope transfer cannot start a
  /// second game beside the direct fallback.
  Future<void> deny();

  /// Stop the runner and everything it started.
  Future<void> killGroup();
}

/// The unified control group from a cgroup file, or "" when it is absent.
/// cgroup v1 lines (a hybrid hierarchy) are skipped; only `0::` is unified.
String readCgroup(String path) {
  try {
    for (final line in File(path).readAsLinesSync()) {
      if (line.startsWith('0::')) return line.substring(3);
    }
  } catch (_) {}
  return '';
}

/// Whether [cgroup] names a systemd service: the launcher's unit. A scope or
/// a terminal is left alone, as starting a scope from one is unnecessary.
bool inServiceCgroup(String cgroup) {
  final last = cgroup.split('/').last;
  return last.endsWith('.service');
}

/// The command that runs [command] behind the gate. `go` on stdin is the
/// permission; `/dev/null` for all three streams before exec keeps the game
/// from taking a pipe for a reader (it then writes hookshot_log.txt).
List<String> gatedCommand(List<String> prefix, List<String> command) => [
      ...prefix,
      '/bin/sh',
      '-c',
      'IFS= read -r ready || exit 1\n'
          '[ "\$ready" = go ] || exit 1\n'
          'exec </dev/null >/dev/null 2>/dev/null\n'
          'exec "\$@"',
      'hookshot-scope',
      ...command,
    ];

/// Where a command is looked up, as shutil.which does for the Tk builder.
String? whichIn(String name, String? path) {
  if (path == null || path.isEmpty) return null;
  for (final dir in path.split(':')) {
    if (dir.isEmpty) continue;
    final file = File('$dir/$name');
    try {
      final stat = file.statSync();
      if (stat.type == FileSystemEntityType.file && (stat.mode & 0x49) != 0) return file.path;
    } catch (_) {}
  }
  return null;
}

class _DartScopedProcess implements ScopedProcess {
  final Process _process;
  bool _exited = false;

  _DartScopedProcess(this._process) {
    unawaited(_process.exitCode.then((_) => _exited = true));
  }

  @override
  int get pid => _process.pid;

  @override
  bool get exited => _exited;

  @override
  Future<int> get exitCode => _process.exitCode;

  @override
  Future<void> allow() async {
    _process.stdin.write('go\n');
    await _process.stdin.flush();
    await _process.stdin.close();
  }

  @override
  Future<void> deny() async {
    await _process.stdin.close();
  }

  @override
  Future<void> killGroup() async {
    try {
      await Process.run('kill', ['-KILL', '-${_process.pid}']);
    } catch (_) {}
    try {
      await _process.exitCode.timeout(const Duration(seconds: 1));
    } catch (_) {}
  }
}

/// Starts the game in a scope of its own when the launcher is a systemd
/// service. Every dependency is injectable so the rules are testable without
/// systemd, a game or a window.
class PlayLauncher {
  final bool linux;
  final String cgroupPath;
  final String procRoot;
  final String? Function(String name, String? path) which;
  final Future<ProcessResult> Function(List<String> command, Map<String, String>? env, Duration timeout) run;
  final Future<ScopedProcess> Function(List<String> command, String cwd, Map<String, String> env) start;
  final String Function(String path) read;
  final DateTime Function() clock;
  final Future<void> Function(Duration) sleep;
  final Duration timeout;
  final Duration poll;

  PlayLauncher({
    bool? linux,
    this.cgroupPath = '/proc/self/cgroup',
    this.procRoot = '/proc',
    this.which = whichIn,
    Future<ProcessResult> Function(List<String> command, Map<String, String>? env, Duration timeout)? run,
    Future<ScopedProcess> Function(List<String> command, String cwd, Map<String, String> env)? start,
    this.read = readCgroup,
    DateTime Function()? clock,
    Future<void> Function(Duration)? sleep,
    this.timeout = const Duration(seconds: 2),
    this.poll = const Duration(milliseconds: 10),
  })  : linux = linux ?? Platform.isLinux,
        run = run ?? _run,
        start = start ?? _start,
        clock = clock ?? DateTime.now,
        sleep = sleep ?? ((duration) => Future<void>.delayed(duration));

  static Future<ProcessResult> _run(List<String> command, Map<String, String>? env, Duration timeout) async {
    final process = await Process.start(command.first, command.sublist(1), environment: env);
    final code = await process.exitCode.timeout(timeout, onTimeout: () {
      process.kill();
      return -1;
    });
    return ProcessResult(process.pid, code, '', '');
  }

  static Future<ScopedProcess> _start(List<String> command, String cwd, Map<String, String> env) async {
    // (setsid makes the runner its own process group, so a timed-out hand-off
    // can be stopped without touching the builder)
    final process = await Process.start('setsid', command, workingDirectory: cwd, environment: env);
    return _DartScopedProcess(process);
  }

  /// The scope command, or [] when this launch must not ask for one.
  List<String> scopePrefix(Map<String, String> env) {
    if (!linux) return const [];
    if (!inServiceCgroup(read(cgroupPath))) return const [];
    final tool = which('systemd-run', env['PATH']);
    if (tool == null) return const [];
    return [tool, ...scopeArguments];
  }

  /// The scope command after the trial run proved the user manager answers.
  Future<List<String>> provenScope(Map<String, String> env) async {
    final prefix = scopePrefix(env);
    if (prefix.isEmpty) return const [];
    try {
      final result = await run([...prefix, 'true'], env, const Duration(seconds: 10));
      return result.exitCode == 0 ? prefix : const [];
    } catch (_) {
      return const [];
    }
  }

  /// Wait until the runner leaves the launcher's cgroup and its descendants.
  /// Only then open the gate. A child that exited, a missing cgroup or the
  /// two-second bound is a failed hand-off, never a reason to guess.
  Future<bool> handOff(ScopedProcess process) async {
    final parent = read('$procRoot/self/cgroup');
    if (parent.isEmpty) return false;
    final deadline = clock().add(timeout);
    while (true) {
      if (process.exited) return false;
      final child = read('$procRoot/${process.pid}/cgroup');
      if (child.isNotEmpty && child != parent && !child.startsWith('$parent/')) {
        try {
          await process.allow();
          return true;
        } catch (_) {
          return false;
        }
      }
      if (!clock().isBefore(deadline)) return false;
      await sleep(poll);
    }
  }

  /// Start [command] scoped when possible, else [direct] exactly once.
  /// Returns whether the scoped hand-off succeeded.
  Future<bool> launch(
    List<String> command, {
    required String cwd,
    required Map<String, String> env,
    required Future<void> Function() direct,
  }) async {
    final prefix = await provenScope(env);
    if (prefix.isNotEmpty) {
      ScopedProcess? process;
      try {
        process = await start(gatedCommand(prefix, command), cwd, env);
        if (await handOff(process)) return true;
      } catch (_) {}
      if (process != null) {
        try {
          await process.deny();
        } catch (_) {}
        await process.killGroup();
      }
    }
    await direct();
    return false;
  }
}
