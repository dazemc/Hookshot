// The Rust engine's one JSON call, exercised through Dart FFI when the
// library is built beside the checkout.
import 'package:flutter_test/flutter_test.dart';

import 'package:hookshot_builder/engine.dart';

void main() {
  test('the Rust engine answers one JSON request', () {
    final engine = RustEngine.open();
    if (engine == null) {
      markTestSkipped('the Rust engine library is not built');
      return;
    }
    final answer = engine.call({'command': 'version.describe'});
    expect(answer['fingerprint'], isA<String>());
    expect((answer['fingerprint'] as String).length, 8);
  });

  test('an unknown command is a typed error', () {
    final engine = RustEngine.open();
    if (engine == null) {
      markTestSkipped('the Rust engine library is not built');
      return;
    }
    expect(
      () => engine.call({'command': 'no.such.command'}),
      throwsA(isA<RustEngineException>()),
    );
  });
}
