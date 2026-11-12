// Builder window state machine: form -> progress -> ready/problem.
// Ports hookshot_builder.py's Builder (minus the Tk widgets): the same modes,
// version check, latest-tag lookup, make_game runs, @@ parsing, Play and Cancel.
import 'dart:async';
import 'dart:io';

import 'package:flutter_bloc/flutter_bloc.dart';
import 'package:equatable/equatable.dart';

import '../backend/build_events.dart';
import '../backend/process.dart';
import '../backend/rust_build.dart';
import '../config.dart' as cfg;
import '../engine.dart';
import '../gamescope.dart';
import '../updater/installer.dart' show latestTag, fetchBytes;
import '../updater/manifest.dart' as upd;

// ---- events ----

sealed class BuilderEvent extends Equatable {
  const BuilderEvent();
  @override
  List<Object?> get props => [];
}

class FormChanged extends BuilderEvent {
  final String? kind;
  final String? nup;
  final String? romfs;
  final String? exefs;
  final String? out;
  final String? language;
  final bool? prepare;
  final bool? dlss;
  final String? track;
  const FormChanged(
      {this.kind, this.nup, this.romfs, this.exefs, this.out, this.language, this.prepare, this.dlss, this.track});
  @override
  List<Object?> get props => [kind, nup, romfs, exefs, out, language, prepare, dlss, track];
}

class SourcePicked extends BuilderEvent {
  const SourcePicked();
}

class DlssSet extends BuilderEvent {
  final bool value;
  const DlssSet(this.value);
  @override
  List<Object?> get props => [value];
}

class CheckRequested extends BuilderEvent {
  const CheckRequested();
}

class BuildRequested extends BuilderEvent {
  const BuildRequested();
}

class MakeUpdateRequested extends BuilderEvent {
  final String folder;
  const MakeUpdateRequested(this.folder);
  @override
  List<Object?> get props => [folder];
}

class BackToForm extends BuilderEvent {
  const BackToForm();
}

class CancelRequested extends BuilderEvent {
  const CancelRequested();
}

class LatestRefreshRequested extends BuilderEvent {
  const LatestRefreshRequested();
}

class _RefreshFormRequested extends BuilderEvent {
  const _RefreshFormRequested();
}

class _LineReceived extends BuilderEvent {
  final String line;
  const _LineReceived(this.line);
  @override
  List<Object?> get props => [line];
}

class _ProcessExited extends BuilderEvent {
  final int code;
  const _ProcessExited(this.code);
  @override
  List<Object?> get props => [code];
}

class _LatestReceived extends BuilderEvent {
  final String? tag;
  const _LatestReceived(this.tag);
  @override
  List<Object?> get props => [tag];
}

class _CheckReceived extends BuilderEvent {
  final String text;
  const _CheckReceived(this.text);
  @override
  List<Object?> get props => [text];
}

// ---- states ----

sealed class BuilderState extends Equatable {
  const BuilderState();
  @override
  List<Object?> get props => [];
}

class BuilderForm extends BuilderState {
  final String kind;
  final String nup;
  final String romfs;
  final String exefs;
  final String out;
  final String language;
  final bool prepare;
  final bool dlss;
  final String track;
  final String checkText;
  final String? latest;
  final double? spaceGb;
  final String buttonLabel;
  final String aboutLabel;
  final bool gameReady;
  final bool sourcePicked;
  final bool dlssToggled;
  final String mode;
  final String statusText;
  const BuilderForm({
    required this.kind,
    required this.nup,
    required this.romfs,
    required this.exefs,
    required this.out,
    required this.language,
    required this.prepare,
    required this.dlss,
    required this.track,
    required this.checkText,
    required this.latest,
    required this.spaceGb,
    required this.buttonLabel,
    required this.aboutLabel,
    required this.gameReady,
    required this.sourcePicked,
    required this.dlssToggled,
    required this.mode,
    this.statusText = '',
  });
  BuilderForm copyWith({
    String? kind,
    String? nup,
    String? romfs,
    String? exefs,
    String? out,
    String? language,
    bool? prepare,
    bool? dlss,
    String? track,
    String? checkText,
    String? Function()? latest,
    double? Function()? spaceGb,
    String? buttonLabel,
    String? aboutLabel,
    bool? gameReady,
    bool? sourcePicked,
    bool? dlssToggled,
    String? mode,
    String? statusText,
  }) =>
      BuilderForm(
        kind: kind ?? this.kind,
        nup: nup ?? this.nup,
        romfs: romfs ?? this.romfs,
        exefs: exefs ?? this.exefs,
        out: out ?? this.out,
        language: language ?? this.language,
        prepare: prepare ?? this.prepare,
        dlss: dlss ?? this.dlss,
        track: track ?? this.track,
        checkText: checkText ?? this.checkText,
        latest: latest != null ? latest() : this.latest,
        spaceGb: spaceGb != null ? spaceGb() : this.spaceGb,
        buttonLabel: buttonLabel ?? this.buttonLabel,
        aboutLabel: aboutLabel ?? this.aboutLabel,
        gameReady: gameReady ?? this.gameReady,
        sourcePicked: sourcePicked ?? this.sourcePicked,
        dlssToggled: dlssToggled ?? this.dlssToggled,
        mode: mode ?? this.mode,
        statusText: statusText ?? this.statusText,
      );
  @override
  List<Object?> get props => [
        kind,
        nup,
        romfs,
        exefs,
        out,
        language,
        prepare,
        dlss,
        track,
        checkText,
        latest,
        spaceGb,
        buttonLabel,
        aboutLabel,
        gameReady,
        sourcePicked,
        dlssToggled,
        mode,
        statusText
      ];
}

class BuildRunning extends BuilderState {
  final String title;
  final String stageText;
  final double? fraction;
  final double? eta;
  final List<String> log;
  final int changed;
  final int compiledFiles;
  final int? totalFiles;
  final DateTime started;
  final DateTime? compileStarted;
  final double? totalSecs;
  final double? compiledSecs;
  final double? previousSecs;
  final bool shadersLater;
  final int damagedCount;
  final String? error;
  final String? built;
  const BuildRunning({
    required this.title,
    required this.stageText,
    required this.fraction,
    required this.eta,
    required this.log,
    required this.changed,
    required this.compiledFiles,
    required this.totalFiles,
    required this.started,
    required this.compileStarted,
    required this.totalSecs,
    required this.compiledSecs,
    required this.previousSecs,
    required this.shadersLater,
    required this.damagedCount,
    required this.error,
    required this.built,
  });
  @override
  List<Object?> get props =>
      [title, stageText, fraction, eta, changed, compiledFiles, totalFiles, shadersLater, damagedCount, error, built];
}

class BuildDone extends BuilderState {
  final String folder;
  final String text;
  final double? compiledSecs;
  final double? previousSecs;
  final bool shadersLater;
  final int damagedCount;
  const BuildDone(
      {required this.folder,
      required this.text,
      required this.compiledSecs,
      required this.previousSecs,
      required this.shadersLater,
      required this.damagedCount});
  @override
  List<Object?> get props => [folder, text, compiledSecs, previousSecs, shadersLater, damagedCount];
}

class BuildFailed extends BuilderState {
  final String error;
  final int damagedCount;
  final List<String> log;
  final bool cancelled;
  const BuildFailed(this.error, {this.damagedCount = 0, this.log = const [], this.cancelled = false});
  @override
  List<Object?> get props => [error, damagedCount, log, cancelled];
}

// ---- bloc ----

class BuilderBloc extends Bloc<BuilderEvent, BuilderState> {
  BuildProcess? _proc;
  bool _cancelling = false;

  /// A build/update subprocess is running (for close confirmation).
  bool get hasRunningProcess => _proc != null;
  StreamSubscription<String>? _lines;
  final Map<String, String> _cfg;

  BuilderBloc({String? launchFolder, String? updateFolder})
      : _cfg = cfg.loadConfig(),
        super(_initialForm(cfg.loadConfig())) {
    on<FormChanged>(_onFormChanged);
    on<SourcePicked>(_onSourcePicked);
    on<DlssSet>(_onDlssSet);
    on<CheckRequested>(_onCheck);
    on<BuildRequested>(_onBuild);
    on<MakeUpdateRequested>(_onMakeUpdate);
    on<CancelRequested>(_onCancel);
    on<BackToForm>((e, emit) {
      emit(BuilderBloc._initialForm(_cfg));
      add(const LatestRefreshRequested());
    });
    on<LatestRefreshRequested>(_onLatest);
    on<_RefreshFormRequested>(_onRefreshForm);
    on<_LineReceived>(_onLine);
    on<_ProcessExited>(_onExit);
    on<_LatestReceived>(_onLatestReceived);
    on<_CheckReceived>(_onCheckReceived);
    if (launchFolder != null) {
      // Launcher mode is handled by LauncherBloc; this bloc stays on the form.
    }
    if (updateFolder != null) {
      // Update mode is handled by UpdateBloc.
    }
    add(const LatestRefreshRequested());
    add(const _RefreshFormRequested());
  }

  static BuilderForm _initialForm(Map<String, String> cfgMap) {
    const languages = cfg.languages;
    final code = cfgMap['language'] ?? 'en-US';
    final name = languages.firstWhere((l) => l.$1 == code, orElse: () => languages.first).$2;
    final out = cfgMap['out'] ?? '${Platform.environment['HOME'] ?? '~'}/Games/Hookshot';
    return BuilderForm(
      kind: cfgMap['kind'] ?? 'nup',
      nup: cfgMap['nup'] ?? '',
      romfs: cfgMap['romfs'] ?? '',
      exefs: cfgMap['exefs'] ?? '',
      out: out,
      language: name,
      prepare: (cfgMap['prepare'] ?? '1') != '0',
      dlss: (cfgMap['dlss'] ?? '0') == '1',
      track: cfg.trackNames.containsKey(cfgMap['track']) ? cfgMap['track']! : upd.trackOf(_toolsTag()),
      checkText: '',
      latest: null,
      spaceGb: cfg.freeGb(_expand(out)),
      buttonLabel: 'Build the game',
      aboutLabel: 'About 45 min',
      gameReady: false,
      sourcePicked: false,
      dlssToggled: false,
      mode: 'build',
    );
  }

  static String _toolsTag() => upd.toolsTag(upd.runningRoot() ?? cfg.toolsRoot());

  String get toolsTag => _toolsTag();

  /// The headers `build.py` needs (Vulkan, SPIR-V, VMA): a source checkout keeps them in
  /// build-linux/deps/cpm, a packaged builder in tools/cpm. The Tk builder and the
  /// AppImage's environment do the same; without it a source build stops at hookshot_vk.so.
  static Map<String, String> _toolEnv() {
    final root = cfg.toolsRoot();
    for (final cpm in ['$root/cpm', '$root/build-linux/deps/cpm']) {
      if (Directory(cpm).existsSync()) return {'RX_CPM': cpm};
    }
    return const {};
  }

  bool get switching {
    final s = state;
    return s is BuilderForm && s.track != upd.trackOf(_toolsTag());
  }

  BuilderForm _refreshedForm(BuilderForm s) {
    final folder = _expand(s.out);
    final gv = upd.gameVersion(folder.isEmpty ? null : folder);
    final kept = gv.kept;
    final game = gv.version;
    final switching = s.track != upd.trackOf(_toolsTag());
    final mode = upd.chooseMode(game, _toolsTag(), s.latest, s.sourcePicked, kept, switchTrack: switching);
    var dlssChanged = false;
    if (mode == 'up-to-date' && kept && game != null && s.sourcePicked && s.dlssToggled) {
      dlssChanged = _folderHasDlss(folder) != s.dlss;
    }
    final effectiveMode = (mode == 'up-to-date' && dlssChanged) ? 'update-game' : mode;
    final target = upd.updateTarget(s.latest, _toolsTag(), switchTrack: switching);
    var button = 'Build the game';
    var about = '';
    var status = '';
    if (effectiveMode == 'update-game') {
      button = 'Update';
      if (dlssChanged && game == target) {
        status = 'Update builds your game again ${s.dlss ? 'with' : 'without'} NVIDIA DLSS.';
      } else {
        status =
            'Hookshot $target is available. Your game is $game. Update downloads only what changed, then compiles what changed.';
      }
    } else if (effectiveMode == 'up-to-date') {
      button = 'Play';
      status = s.latest == null
          ? 'Your game is $game, as this builder. GitHub did not answer, so newer versions could not be checked.'
          : '$game is the latest version.';
    } else if (effectiveMode == 'update-builder') {
      button = 'Update builder';
      final have = _toolsTag();
      status =
          'Hookshot Builder $target is available (this one is ${have.isEmpty ? 'this one' : have}). Update it first, then build.';
    }
    if (effectiveMode == 'build') about = 'About 45 min';
    return s.copyWith(
      mode: effectiveMode,
      buttonLabel: button,
      aboutLabel: about,
      statusText: status,
      gameReady: upd.gameReady(_expand(_cfg['out'] ?? s.out)),
    );
  }

  void _onRefreshForm(_RefreshFormRequested e, Emitter<BuilderState> emit) {
    final s = state;
    if (s is BuilderForm) emit(_refreshedForm(s));
  }

  static String _expand(String folder) {
    var f = folder.trim();
    if (f.startsWith('~/')) f = '${Platform.environment['HOME'] ?? '~'}${f.substring(1)}';
    return f;
  }

  static bool _folderHasDlss(String folder) {
    if (folder.isEmpty) return false;
    try {
      final hasRuntime = Directory('$folder/ngx').listSync().any((e) {
        final name = e.path.split(RegExp(r'[/\\]')).last;
        return name.startsWith('libnvidia-ngx-dlss.so') || name == 'nvngx_dlss.dll';
      });
      final sdk = '${dlssSdkDir()}/LICENSE.txt';
      return hasRuntime && File(sdk).existsSync();
    } catch (_) {
      return false;
    }
  }

  void _onFormChanged(FormChanged e, Emitter<BuilderState> emit) {
    final s = state;
    if (s is! BuilderForm) return;
    emit(s.copyWith(
      kind: e.kind,
      nup: e.nup,
      romfs: e.romfs,
      exefs: e.exefs,
      out: e.out,
      language: e.language,
      prepare: e.prepare,
      dlss: e.dlss,
      track: e.track,
      spaceGb: () => cfg.freeGb(_expand(e.out ?? s.out)),
    ));
    final ns = state;
    if (ns is BuilderForm) emit(_refreshedForm(ns));
  }

  void _onSourcePicked(SourcePicked e, Emitter<BuilderState> emit) {
    final s = state;
    if (s is! BuilderForm) return;
    emit(s.copyWith(sourcePicked: true));
    final ns = state;
    if (ns is BuilderForm) emit(_refreshedForm(ns));
  }

  void _onDlssSet(DlssSet e, Emitter<BuilderState> emit) {
    final s = state;
    if (s is! BuilderForm) return;
    emit(s.copyWith(dlss: e.value, dlssToggled: true));
    _cfg['dlss'] = e.value ? '1' : '0';
    cfg.saveConfig(_cfg);
    final ns = state;
    if (ns is BuilderForm) emit(_refreshedForm(ns));
  }

  Future<void> _onCheck(CheckRequested e, Emitter<BuilderState> emit) async {
    final s = state;
    if (s is! BuilderForm) return;
    emit(s.copyWith(checkText: 'Checking the game version...'));
    final engine = RustEngine.open();
    if (engine == null) {
      add(const _CheckReceived('The version could not be read (the Rust engine is missing).'));
      return;
    }
    try {
      final answer = engine.call({
        'command': 'build.check',
        'nup': s.kind == 'nup' ? s.nup : '',
        'exefs': s.kind == 'nup' ? '' : s.exefs,
        'work': '${cfg.toolsRoot()}/work',
      });
      add(_CheckReceived(answer['message'] as String? ?? 'The version could not be read.'));
    } catch (error) {
      add(_CheckReceived('The version could not be read: $error'));
    }
  }

  void _onCheckReceived(_CheckReceived e, Emitter<BuilderState> emit) {
    final s = state;
    if (s is! BuilderForm) return;
    emit(s.copyWith(checkText: e.text));
  }

  Future<void> _onLatest(LatestRefreshRequested e, Emitter<BuilderState> emit) async {
    final forced = Platform.environment['HOOKSHOT_LATEST_TAG'];
    if (forced == 'none') {
      add(const _LatestReceived(null));
      return;
    }
    final s = state;
    final track = s is BuilderForm ? s.track : upd.trackOf(_toolsTag());
    try {
      final tag = forced ?? await latestTag(fetchBytes, track: track);
      add(_LatestReceived(tag));
    } catch (_) {
      add(const _LatestReceived(null));
    }
  }

  void _onLatestReceived(_LatestReceived e, Emitter<BuilderState> emit) {
    final s = state;
    if (s is! BuilderForm) return;
    emit(s.copyWith(latest: () => e.tag));
    final ns = state;
    if (ns is BuilderForm) emit(_refreshedForm(ns));
  }

  Future<void> _onBuild(BuildRequested e, Emitter<BuilderState> emit) async {
    final s = state;
    if (s is! BuilderForm) return;
    try {
      requireDlssSupported(dlss: s.dlss);
    } on DlssBuildUnsupported catch (error) {
      emit(BuildFailed(error.toString()));
      return;
    }
    // update-game / update-builder modes are handled by UpdateBloc via the UI.
    final out = _expand(s.out);
    if (s.kind == 'nup') {
      if (!File(s.nup).existsSync()) return;
    } else {
      if (!Directory(s.romfs).existsSync() || !Directory(s.exefs).existsSync()) return;
    }
    if (out.isEmpty) return;
    final free = cfg.freeGb(out);
    if (free != null && free < 45) {
      emit(BuildFailed(
        'There is not enough free space for the build. $out has ${free.toStringAsFixed(1)} GB free; about 45 GB is needed '
        '(the game files, the translated code and the compiled objects). Free some space or choose another install folder.',
      ));
      return;
    }
    final code = cfg.languages.firstWhere((l) => l.$2 == s.language, orElse: () => cfg.languages.first).$1;
    _cfg.updateAll((k, v) => v);
    _cfg['kind'] = s.kind;
    _cfg['nup'] = s.nup;
    _cfg['romfs'] = s.romfs;
    _cfg['exefs'] = s.exefs;
    _cfg['out'] = out;
    _cfg['language'] = code;
    _cfg['prepare'] = s.prepare ? '1' : '0';
    _cfg['dlss'] = s.dlss ? '1' : '0';
    cfg.saveConfig(_cfg);
    if (rustNupApplies(kind: s.kind)) {
      final work = rustBuildWork();
      late final String rust;
      try {
        rust = requireRustEngineCli();
        final exitCode = await _runPrepare(
          emit,
          [rust, '--stream'],
          rustPrepareRequest(nup: s.nup, out: out, work: work),
          title: 'Building your game',
        );
        if (exitCode < 0) return;
        if (exitCode != 0) {
          _failPrepare(emit);
          return;
        }
      } on RustEngineMissing catch (error) {
        emit(BuildFailed(error.toString()));
        return;
      }
      if (_cancelling) {
        _cancelling = false;
        emit(const BuildFailed('Build cancelled. Your game folder was not replaced.', cancelled: true));
        return;
      }
      // The engine already has the exefs and romfs it read: hand them straight
      // to build.all so the lift, compile, link and package stay in Rust,
      // NVIDIA DLSS included.
      if (rustNupBuildApplies(cli: rust)) {
        _startProcess(
          emit,
          [rust, '--stream'],
          stdinText: rustNupBuildRequest(
            out: out,
            work: work,
            language: code,
            prepare: s.prepare,
            dlss: s.dlss,
            ngx: s.dlss ? dlssSdkDir() : '',
            vk: Platform.environment['HOOKSHOT_VK_PREBUILT'] ?? '',
          ),
          title: 'Building your game',
        );
        return;
      }
    }
    // An unpacked folder can go through the Rust build.all engine, DLSS too.
    final rust = rustEngineCli();
    if (rust != null && rustBuildApplies(kind: s.kind, cli: rust)) {
      _startProcess(
        emit,
        [rust, '--stream'],
        stdinText: rustBuildRequest(
          exefs: s.exefs,
          romfs: s.romfs,
          out: out,
          work: rustBuildWork(),
          vk: Platform.environment['HOOKSHOT_VK_PREBUILT'] ?? '',
          language: code,
          prepare: s.prepare,
          dlss: s.dlss,
          ngx: s.dlss ? dlssSdkDir() : '',
        ),
        title: 'Building your game',
      );
      return;
    }
    // No Python fallback: the Rust engine is the builder.
    const missing = RustEngineMissing(
      'The Rust engine is missing: builds need it, and the builder no longer falls back to make_game.py. '
      'Install Hookshot Builder again.',
    );
    emit(BuildFailed(missing.toString()));
  }

  Future<void> _onMakeUpdate(MakeUpdateRequested e, Emitter<BuilderState> emit) async {
    if (e.folder.isEmpty) return;
    final s = state;
    final prepare = s is BuilderForm ? s.prepare : true;
    final dlss = s is BuilderForm ? s.dlss : _folderHasDlss(e.folder);
    try {
      requireDlssSupported(dlss: dlss);
    } on DlssBuildUnsupported catch (error) {
      emit(BuildFailed(error.toString()));
      return;
    }
    final rust = rustEngineCli();
    if (rust != null) {
      if (!rustUpdateReady(e.folder)) {
        emit(const BuildFailed(
            'This game folder has no copy of the game files to update from: it was built before updates came in. '
            'Choose your game files in Hookshot Builder and build into the same folder once; updates after that work '
            'by themselves.'));
        return;
      }
      _startProcess(
        emit,
        [rust, '--stream'],
        stdinText: rustUpdateRequest(
          folder: e.folder,
          language: folderLanguage(e.folder),
          prepare: prepare,
          dlss: dlss,
          ngx: dlss ? dlssSdkDir() : '',
          vk: Platform.environment['HOOKSHOT_VK_PREBUILT'] ?? '',
        ),
        title: 'Updating your game',
      );
      return;
    }
    // No Python fallback: the Rust engine is the updater.
    const missing = RustEngineMissing(
      'The Rust engine is missing: updates need it, and the builder no longer falls back to make_game.py. '
      'Install Hookshot Builder again.',
    );
    emit(BuildFailed(missing.toString()));
  }

  void _emitRunning(Emitter<BuilderState> emit, String title, String stageText) {
    emit(BuildRunning(
      title: title,
      stageText: stageText,
      fraction: null,
      eta: null,
      log: const [],
      changed: -1,
      compiledFiles: 0,
      totalFiles: null,
      started: DateTime.now(),
      compileStarted: null,
      totalSecs: null,
      compiledSecs: null,
      previousSecs: null,
      shadersLater: false,
      damagedCount: 0,
      error: null,
      built: null,
    ));
  }

  Future<void> _startProcess(Emitter<BuilderState> emit, List<String> cmd,
      {Map<String, String>? env, required String title, String? stdinText}) async {
    _cancelling = false;
    await _lines?.cancel();
    if (_proc != null) {
      try {
        await _proc!.killGroup();
      } catch (_) {}
      _proc = null;
    }
    _emitRunning(emit, title, 'Starting');
    try {
      final merged = Map<String, String>.from(env ?? Platform.environment)..addAll(_toolEnv());
      _proc = await BuildProcess.start(cmd, cwd: cfg.toolsRoot(), env: merged, stdinText: stdinText);
    } catch (err) {
      emit(BuildFailed('$err'));
      return;
    }
    _lines = _proc!.lines
        .listen((line) => add(_LineReceived(line)), onDone: () async => add(_ProcessExited(await _proc!.exitCode)));
  }

  /// The NUP read stage: run the engine's `build.prepare` to completion while
  /// its `@@` lines keep the progress page alive. Returns its exit code, or -1
  /// when the process could not start (the failure state was already emitted).
  Future<int> _runPrepare(Emitter<BuilderState> emit, List<String> cmd, String request, {required String title}) async {
    _cancelling = false;
    await _lines?.cancel();
    if (_proc != null) {
      try {
        await _proc!.killGroup();
      } catch (_) {}
      _proc = null;
    }
    _emitRunning(emit, title, 'Reading your game files');
    final BuildProcess process;
    try {
      final merged = Map<String, String>.from(Platform.environment)..addAll(_toolEnv());
      process = await BuildProcess.start(cmd, cwd: cfg.toolsRoot(), env: merged, stdinText: request);
      _proc = process;
    } catch (err) {
      emit(BuildFailed('$err'));
      return -1;
    }
    final done = Completer<int>();
    _lines = process.lines.listen((line) => _applyLine(line, emit), onDone: () async {
      if (!done.isCompleted) done.complete(await process.exitCode);
    });
    final code = await done.future;
    _proc = null;
    return code;
  }

  void _failPrepare(Emitter<BuilderState> emit) {
    final s = state;
    final log = s is BuildRunning ? s.log : const <String>[];
    if (_cancelling) {
      _cancelling = false;
      emit(BuildFailed('Build cancelled. Your game folder was not replaced.', log: log, cancelled: true));
      return;
    }
    final last = log.isEmpty ? null : log.last.trim();
    final error = s is BuildRunning ? s.error : null;
    emit(BuildFailed(
      error ??
          (last != null && last.isNotEmpty
              ? last
              : 'The Rust engine could not read your game files. See the log for details.'),
      log: log,
    ));
  }

  static const _stages = {
    'read': 'Reading your game files',
    'lift': 'Translating the game to PC code',
    'compile': 'Compiling',
    'finish': 'Putting the game folder together',
    'shaders': 'Preparing shaders'
  };

  void _onLine(_LineReceived e, Emitter<BuilderState> emit) => _applyLine(e.line, emit);

  void _applyLine(String line, Emitter<BuilderState> emit) {
    final s = state;
    if (s is! BuildRunning) return;
    final event = parseBuilderLine(line);
    final log = [...s.log];
    if (event is LogEvent) log.add(event.line);
    String stageText = s.stageText;
    double? fraction = s.fraction;
    double? eta = s.eta;
    var changed = s.changed;
    var compiledFiles = s.compiledFiles;
    int? totalFiles = s.totalFiles;
    DateTime? compileStarted = s.compileStarted;
    double? totalSecs = s.totalSecs;
    double? compiledSecs = s.compiledSecs;
    double? previousSecs = s.previousSecs;
    var shadersLater = s.shadersLater;
    var damagedCount = s.damagedCount;
    var error = s.error;
    String? built = s.built;
    final started = s.started;
    switch (event) {
      case StageEvent(id: final id, text: final text):
        stageText = _stages[id] ?? text;
        log.add('— $stageText');
        fraction = 0;
        eta = null;
        compileStarted = id == 'compile' ? DateTime.now() : null;
      case ProgressEvent(fraction: final f):
        fraction = f;
      case PlanEvent(changed: final c, total: final t):
        changed = c;
        totalFiles = t;
        fraction = c == 0 ? 1 : 0;
        log.add(c == 0 ? 'All $t files are already built' : 'Compiling $c of $t files');
        stageText = c == 0
            ? 'Compiling: everything is already built'
            : c < t
                ? 'Compiling $c changed files of $t'
                : 'Compiling $t files';
      case FileEvent(name: final file, seconds: final seconds, kind: final kind):
        log.add(kind == 'cached' ? 'cached $file' : 'built $file (${seconds.toStringAsFixed(1)} s)');
        if (kind == 'new' || kind == 'again') {
          compiledFiles++;
          if (changed > 0) {
            stageText = 'Compiling $compiledFiles of $changed changed files';
            fraction = (compiledFiles / changed).clamp(0.0, 1.0);
          }
        }
      case EtaEvent(seconds: final v):
        eta = v;
      case TimesEvent(total: final a, compiled: final b, previous: final c):
        totalSecs = a;
        compiledSecs = b;
        previousSecs = c;
      case DoneEvent(folder: final f):
        built = f;
      case ErrorEvent(text: final t):
        error = t;
      case NotPreparedEvent():
        shadersLater = true;
      case DamagedEvent(count: final c):
        damagedCount = c;
      case LogEvent():
        break;
    }
    final capped = log.length > 3000 ? log.sublist(log.length - 3000) : log;
    emit(BuildRunning(
      title: s.title,
      stageText: stageText,
      fraction: fraction,
      eta: eta,
      log: capped,
      changed: changed,
      compiledFiles: compiledFiles,
      totalFiles: totalFiles,
      started: started,
      compileStarted: compileStarted,
      totalSecs: totalSecs,
      compiledSecs: compiledSecs,
      previousSecs: previousSecs,
      shadersLater: shadersLater,
      damagedCount: damagedCount,
      error: error,
      built: built,
    ));
  }

  void _onExit(_ProcessExited e, Emitter<BuilderState> emit) {
    final s = state;
    if (s is! BuildRunning) return;
    final folder = s.built;
    _proc = null;
    if (e.code == 0 && folder != null && upd.gameReady(folder)) {
      final elapsed = DateTime.now().difference(s.started);
      var text = 'Your game is ready, built in ${_clock(elapsed)}';
      if (s.compiledSecs != null) {
        text += ', compiled in ${_clock(Duration(seconds: s.compiledSecs!.toInt()))}';
        if ((s.previousSecs ?? -1) > 0) text += ' · last time ${_clock(Duration(seconds: s.previousSecs!.toInt()))}';
      }
      if (s.shadersLater) text += ' · the shaders are prepared at its first start';
      if (s.damagedCount > 0) {
        text +=
            ' · ${s.damagedCount} damaged game file(s) replaced with stand-ins (see hookshot_damaged_files.txt beside the game)';
      }
      emit(BuildDone(
          folder: folder,
          text: text,
          compiledSecs: s.compiledSecs,
          previousSecs: s.previousSecs,
          shadersLater: s.shadersLater,
          damagedCount: s.damagedCount));
    } else if (_cancelling) {
      _cancelling = false;
      emit(BuildFailed('Build cancelled. Your game folder was not replaced.', log: s.log, cancelled: true));
    } else {
      final last = s.log.isEmpty ? null : s.log.last.trim();
      emit(BuildFailed(
          s.error ?? (last != null && last.isNotEmpty ? last : 'The build stopped. See the log for details.'),
          damagedCount: s.damagedCount,
          log: s.log));
    }
  }

  static String _clock(Duration d) =>
      '${d.inHours}:${(d.inMinutes % 60).toString().padLeft(2, '0')}:${(d.inSeconds % 60).toString().padLeft(2, '0')}';

  Future<void> _onCancel(CancelRequested e, Emitter<BuilderState> emit) async {
    if (_proc != null) {
      _cancelling = true;
      try {
        await _proc!.killGroup();
      } catch (_) {}
    }
  }

  /// Start the built game detached, without the builder's libraries, as the
  /// Tk Play button and hookshot-launch do.
  static Future<void> playFolder(String folder, {String? profile, bool? gamescope}) async {
    final exe = upd.gameExe(folder);
    if (exe == null) return;
    final env = Map<String, String>.from(Platform.environment);
    for (final key in env.keys.toList()) {
      if (key.startsWith('HOOKSHOT_') ||
          key.startsWith('PYTHON') ||
          key.startsWith('APPDIR') ||
          key.startsWith('APPIMAGE') ||
          key.startsWith('ARGV0') ||
          key.startsWith('OWD') ||
          key == 'LD_LIBRARY_PATH') {
        env.remove(key);
      }
    }
    env['HOOKSHOT_NO_UPDATE_CHECK'] = '1';
    if (profile != null && profile.isNotEmpty) env['HOOKSHOT_PROFILE'] = profile;
    final settings = GamescopeSettings.load();
    final forced = Platform.environment['HOOKSHOT_GAMESCOPE'];
    if (forced != null) settings.enabled = forced != '0' && forced.isNotEmpty;
    if (gamescope != null) settings.enabled = gamescope;
    final gamescopeExe = settings.enabled ? _which('gamescope') : null;
    if (gamescopeExe != null) {
      final command = settings.command(exe);
      final prefix = settings.gameMode ? _which('gamemoderun') : null;
      await Process.start(
        prefix ?? gamescopeExe,
        prefix != null ? [gamescopeExe, ...command] : command,
        workingDirectory: folder,
        environment: env,
        mode: ProcessStartMode.detached,
      );
      return;
    }
    await Process.start(exe, [], workingDirectory: folder, environment: env, mode: ProcessStartMode.detached);
  }

  /// The game already runs inside gamescope (Steam Deck game mode, or a
  /// nested session): starting another would not work.
  static bool _insideGamescope() =>
      (Platform.environment['GAMESCOPE_WAYLAND_DISPLAY'] ?? '').isNotEmpty ||
      (Platform.environment['XDG_CURRENT_DESKTOP'] ?? '').toLowerCase().contains('gamescope');

  static String? _which(String name) {
    if (!Platform.isLinux || _insideGamescope()) return null;
    try {
      final which = Process.runSync('which', [name]);
      final path = (which.stdout as String).trim();
      if (which.exitCode == 0 && path.isNotEmpty) return path;
    } catch (_) {}
    return null;
  }

  @override
  Future<void> close() async {
    await _lines?.cancel();
    return super.close();
  }
}

/// Countdown text, as hookshot_builder.py left_text.
String leftText(double seconds) {
  if (seconds < 45) return 'less than a minute left';
  if (seconds < 90) return 'about a minute left';
  final minutes = (seconds / 60).round();
  return minutes >= 60 ? 'about ${minutes ~/ 60} h ${minutes % 60} min left' : 'about $minutes minutes left';
}
