// Rust engine FFI. One JSON call, one JSON answer; the Python helpers remain
// only until their Rust replacements are wired.
import 'dart:convert';
import 'dart:ffi';
import 'dart:io';

import 'package:ffi/ffi.dart';
import 'package:path/path.dart' as p;

import 'config.dart' as cfg;

typedef _CallNative = Pointer<Utf8> Function(Pointer<Utf8>);
typedef _CallDart = Pointer<Utf8> Function(Pointer<Utf8>);
typedef _FreeNative = Void Function(Pointer<Utf8>);
typedef _FreeDart = void Function(Pointer<Utf8>);

class RustEngineException implements Exception {
  final String message;
  const RustEngineException(this.message);
  @override
  String toString() => message;
}

/// The Rust engine is not installed where the builder looks. A .nup build's
/// read stage is the engine's now, so it stops rather than falling back to a
/// Python path that no longer exists.
class RustEngineMissing implements Exception {
  final String message;
  const RustEngineMissing([
    this.message = 'The Rust engine is missing: reading a .nup package needs it. '
        'Install Hookshot Builder again, or choose an extracted romfs and exefs folder.',
  ]);
  @override
  String toString() => message;
}

class RustEngine {
  final Pointer<Utf8> Function(Pointer<Utf8>) _call;
  final void Function(Pointer<Utf8>) _free;
  const RustEngine._(this._call, this._free);

  static RustEngine? _instance;
  static bool _tried = false;

  /// Opens the engine once. Null when the library is not built next to the
  /// app; callers keep their existing path until it is.
  static RustEngine? open() {
    if (_tried) return _instance;
    _tried = true;
    for (final candidate in _candidates()) {
      try {
        final lib = DynamicLibrary.open(candidate);
        final call = lib.lookupFunction<_CallNative, _CallDart>('hookshot_engine_call');
        final free = lib.lookupFunction<_FreeNative, _FreeDart>('hookshot_engine_free');
        _instance = RustEngine._(call, free);
        return _instance;
      } catch (_) {}
    }
    return null;
  }

  static List<String> _candidates() {
    final env = Platform.environment['HOOKSHOT_ENGINE_LIB'];
    final exe = File(Platform.resolvedExecutable).parent.path;
    final root = cfg.toolsRoot();
    final name = Platform.isWindows ? 'hookshot_engine.dll' : 'libhookshot_engine.so';
    return [
      if (env != null && env.isNotEmpty) env,
      p.join(exe, name),
      p.join(exe, 'lib', name),
      p.join(root, 'builder', 'rust', 'target', 'release', name),
      p.join(root, 'builder', 'rust', 'target', 'debug', name),
    ];
  }

  /// Run one command, e.g. `{"command":"profiles.list","folder":"..."}`.
  Map<String, dynamic> call(Map<String, Object?> request) {
    final text = jsonEncode(request);
    final input = text.toNativeUtf8();
    Pointer<Utf8> output = nullptr;
    try {
      output = _call(input);
      final response = output.toDartString();
      final value = jsonDecode(response);
      if (value is! Map<String, dynamic>) {
        throw const RustEngineException('The Rust engine returned no object.');
      }
      if (value['error'] is String) throw RustEngineException(value['error'] as String);
      return value;
    } on RustEngineException {
      rethrow;
    } catch (error) {
      throw RustEngineException('The Rust engine failed: $error');
    } finally {
      calloc.free(input);
      if (output != nullptr) _free(output);
    }
  }
}
