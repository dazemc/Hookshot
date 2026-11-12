// Accessibility helpers: reduced-motion timing and status announcements.
import 'package:fluent_ui/fluent_ui.dart';

/// Motion rules shared by the builder. When the platform asks for reduced
/// motion, transitions happen at once instead of sliding or popping.
abstract final class HsMotion {
  static bool reduced(BuildContext context) => MediaQuery.maybeOf(context)?.disableAnimations ?? false;

  static Duration duration(BuildContext context, int milliseconds) =>
      reduced(context) ? Duration.zero : Duration(milliseconds: milliseconds);
}

/// Wraps visible status text in a polite live region, so a screen reader
/// announces it when [text] changes. The visible child is hidden from the
/// semantics tree to avoid reading the same sentence twice.
class HsLiveStatus extends StatelessWidget {
  final String text;
  final Widget child;
  const HsLiveStatus({super.key, required this.text, required this.child});

  @override
  Widget build(BuildContext context) {
    return Semantics(
      container: true,
      liveRegion: true,
      label: text,
      child: ExcludeSemantics(child: child),
    );
  }
}

/// Semantics for a progress bar: one label, a percentage value and an
/// optional live-region flag. The visual indicator is excluded so assistive
/// technology reads the builder's wording rather than a raw number.
class HsProgressSemantics extends StatelessWidget {
  final String label;
  final double? percent;
  final String? detail;
  final bool announce;
  final Widget child;
  const HsProgressSemantics({
    super.key,
    required this.label,
    required this.percent,
    required this.child,
    this.detail,
    this.announce = false,
  });

  @override
  Widget build(BuildContext context) {
    final value = percent == null ? 'working' : '${percent!.clamp(0, 100).round()}%';
    return Semantics(
      container: true,
      liveRegion: announce,
      label: detail == null ? label : '$label. $detail',
      value: value,
      child: ExcludeSemantics(child: child),
    );
  }
}
