import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:hookshot_builder/play_scope.dart';

const _service = '/user.slice/user-1000.slice/user@1000.service/app.slice/app-hookshot@0123.service';
const _scope = '/user.slice/user-1000.slice/user@1000.service/app.slice/app-gnome-hookshot-4242.scope';
const _tool = '/usr/bin/systemd-run';
const _prefix = [_tool, '--user', '--scope', '--collect', '--quiet', '--'];

class FakeScopedProcess implements ScopedProcess {
  @override
  final int pid = 4242;
  bool _exited = false;
  bool allowed = false;
  bool denied = false;
  bool killed = false;

  @override
  bool get exited => _exited;

  void exit() => _exited = true;

  @override
  Future<int> get exitCode async => _exited ? 1 : 0;

  @override
  Future<void> allow() async => allowed = true;

  @override
  Future<void> deny() async => denied = true;

  @override
  Future<void> killGroup() async {
    killed = true;
    _exited = true;
  }
}

PlayLauncher launcher({
  bool linux = true,
  String cgroup = _service,
  String? tool = _tool,
  Future<ProcessResult> Function(List<String>, Map<String, String>?, Duration)? run,
  Future<ScopedProcess> Function(List<String>, String, Map<String, String>)? start,
  String Function(String)? read,
  DateTime Function()? clock,
  Future<void> Function(Duration)? sleep,
  Duration timeout = const Duration(seconds: 2),
  Duration poll = const Duration(milliseconds: 10),
}) {
  return PlayLauncher(
    linux: linux,
    cgroupPath: '/proc/self/cgroup',
    procRoot: '/proc',
    which: (name, path) => name == 'systemd-run' ? tool : null,
    run: run ?? ((command, env, timeout) async => ProcessResult(1, 0, '', '')),
    start: start ?? ((command, cwd, env) async => FakeScopedProcess()),
    read: read ?? ((path) => path.endsWith('/self/cgroup') ? cgroup : ''),
    clock: clock ?? DateTime.now,
    sleep: sleep ?? ((duration) async {}),
    timeout: timeout,
    poll: poll,
  );
}

void main() {
  group('cgroup rules', () {
    test('the unified line is read from a hybrid hierarchy', () {
      final dir = Directory.systemTemp.createTempSync('hookshot-cgroup-');
      addTearDown(() => dir.deleteSync(recursive: true));
      final file = File('${dir.path}/cgroup')
        ..writeAsStringSync('12:pids:/user.slice\n1:name=systemd:/user.slice\n0::$_service\n');
      expect(readCgroup(file.path), _service);
      expect(readCgroup('${dir.path}/missing'), '');
    });

    test('only a service asks for a scope', () {
      expect(inServiceCgroup(_service), isTrue);
      expect(inServiceCgroup(_scope), isFalse);
      expect(inServiceCgroup('/'), isFalse);
      expect(inServiceCgroup(''), isFalse);
    });

    test('the gate waits for go and restores /dev/null before exec', () {
      final command = gatedCommand(_prefix, ['/games/hookshot']);
      expect(command.sublist(0, _prefix.length), _prefix);
      expect(command.sublist(_prefix.length, _prefix.length + 3), ['/bin/sh', '-c', command[_prefix.length + 2]]);
      expect(command[_prefix.length + 2], contains('IFS= read -r ready'));
      expect(command[_prefix.length + 2], contains('exec </dev/null >/dev/null 2>/dev/null'));
      expect(command.sublist(_prefix.length + 3), ['hookshot-scope', '/games/hookshot']);
    });
  });

  group('scope choice', () {
    test('non-Linux, a scope, no systemd-run and no cgroup ask for nothing', () {
      expect(launcher(linux: false).scopePrefix({'PATH': '/usr/bin'}), isEmpty);
      expect(launcher(cgroup: _scope).scopePrefix({'PATH': '/usr/bin'}), isEmpty);
      expect(launcher(cgroup: '/').scopePrefix({'PATH': '/usr/bin'}), isEmpty);
      expect(launcher(tool: null).scopePrefix({'PATH': '/usr/bin'}), isEmpty);
      expect(launcher(read: (_) => '').scopePrefix({'PATH': '/usr/bin'}), isEmpty);
    });

    test('a service with systemd-run asks for a scope', () {
      expect(launcher().scopePrefix({'PATH': '/usr/bin'}), _prefix);
    });

    test('the trial decides whether the scope is used', () async {
      var calls = 0;
      expect(
        await launcher(run: (command, env, timeout) async {
          calls++;
          expect(command, [..._prefix, 'true']);
          expect(env, {'PATH': '/usr/bin'});
          expect(timeout, const Duration(seconds: 10));
          return ProcessResult(1, 0, '', '');
        }).provenScope({'PATH': '/usr/bin'}),
        _prefix,
      );
      expect(calls, 1);
      expect(
        await launcher(run: (command, env, timeout) async => ProcessResult(1, 1, '', ''))
            .provenScope({'PATH': '/usr/bin'}),
        isEmpty,
      );
      expect(
        await launcher(run: (command, env, timeout) async => throw const OSError('exec'))
            .provenScope({'PATH': '/usr/bin'}),
        isEmpty,
      );
    });
  });

  group('hand-off', () {
    test('a child that left the service opens the gate', () async {
      final process = FakeScopedProcess();
      final play = launcher(
        read: (path) => path.endsWith('/self/cgroup')
            ? _service
            : '${_service.split('/app.slice/').first}/app.slice/run-4242.scope',
      );
      expect(await play.handOff(process), isTrue);
      expect(process.allowed, isTrue);
    });

    test('a child still in the service times out without permission', () async {
      final process = FakeScopedProcess();
      var now = DateTime.utc(2026, 1, 1);
      final play = launcher(
        read: (path) => _service,
        clock: () => now,
        sleep: (duration) async => now = now.add(duration),
        timeout: const Duration(milliseconds: 20),
        poll: const Duration(milliseconds: 10),
      );
      expect(await play.handOff(process), isFalse);
      expect(process.allowed, isFalse);
    });

    test('a missing parent or an exited child is a failed hand-off', () async {
      expect(await launcher(read: (_) => '').handOff(FakeScopedProcess()), isFalse);
      final exited = FakeScopedProcess()..exit();
      expect(await launcher().handOff(exited), isFalse);
    });
  });

  group('launch', () {
    test('no scope starts the command directly once', () async {
      var direct = 0;
      var started = 0;
      final ok = await launcher(
          linux: false,
          start: (command, cwd, env) async {
            started++;
            return FakeScopedProcess();
          }).launch(['/games/hookshot'], cwd: '/games', env: {'PATH': '/usr/bin'}, direct: () async => direct++);
      expect(ok, isFalse);
      expect(direct, 1);
      expect(started, 0);
    });

    test('a proven hand-off starts nothing directly', () async {
      var direct = 0;
      FakeScopedProcess? started;
      final ok = await launcher(
        start: (command, cwd, env) async {
          expect(command.sublist(0, _prefix.length), _prefix);
          expect(command.sublist(_prefix.length, _prefix.length + 3), ['/bin/sh', '-c', command[_prefix.length + 2]]);
          expect(command.last, '/games/hookshot');
          started = FakeScopedProcess();
          return started!;
        },
        read: (path) => path.endsWith('/self/cgroup')
            ? _service
            : '${_service.split('/app.slice/').first}/app.slice/run-4242.scope',
      ).launch(['/games/hookshot'], cwd: '/games', env: {'PATH': '/usr/bin'}, direct: () async => direct++);
      expect(ok, isTrue);
      expect(direct, 0);
      expect(started!.allowed, isTrue);
      expect(started!.killed, isFalse);
    });

    test('a timed-out hand-off denies, reaps and falls back exactly once', () async {
      var direct = 0;
      var now = DateTime.utc(2026, 1, 1);
      final process = FakeScopedProcess();
      final ok = await launcher(
        start: (command, cwd, env) async => process,
        read: (path) => _service,
        clock: () => now,
        sleep: (duration) async => now = now.add(duration),
        timeout: const Duration(milliseconds: 20),
        poll: const Duration(milliseconds: 10),
      ).launch(['/games/hookshot'], cwd: '/games', env: {'PATH': '/usr/bin'}, direct: () async => direct++);
      expect(ok, isFalse);
      expect(direct, 1);
      expect(process.allowed, isFalse);
      expect(process.denied, isTrue);
      expect(process.killed, isTrue);
    });

    test('a failed start falls back directly once', () async {
      var direct = 0;
      final ok = await launcher(start: (command, cwd, env) async => throw const OSError('exec'))
          .launch(['/games/hookshot'], cwd: '/games', env: {'PATH': '/usr/bin'}, direct: () async => direct++);
      expect(ok, isFalse);
      expect(direct, 1);
    });

    test('a failed trial starts the command directly once', () async {
      var direct = 0;
      final ok = await launcher(
        run: (command, env, timeout) async => ProcessResult(1, 1, '', ''),
      ).launch(['/games/hookshot'], cwd: '/games', env: {'PATH': '/usr/bin'}, direct: () async => direct++);
      expect(ok, isFalse);
      expect(direct, 1);
    });
  });
}
