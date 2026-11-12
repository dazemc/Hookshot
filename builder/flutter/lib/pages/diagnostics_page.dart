// The diagnostics dialog shared by the launcher and the problem page.
import 'package:fluent_ui/fluent_ui.dart';
import 'package:flutter/services.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../diagnostics.dart';
import '../play.dart';
import '../theme.dart';

Future<void> showDiagnostics(BuildContext context, String folder) async {
  Map<String, Object?>? report;
  Object? error;
  try {
    report = collect(folder);
    if (report == null) error = 'The Rust engine library is not built. Run builder/rust/build.sh.';
  } catch (e) {
    error = e;
  }
  if (!context.mounted) return;
  await showDialog<void>(
    context: context,
    builder: (context) => _DiagnosticsDialog(folder: folder, report: report, error: error),
  );
}

class _DiagnosticsDialog extends StatefulWidget {
  final String folder;
  final Map<String, Object?>? report;
  final Object? error;
  const _DiagnosticsDialog({required this.folder, this.report, this.error});
  @override
  State<_DiagnosticsDialog> createState() => _DiagnosticsDialogState();
}

class _DiagnosticsDialogState extends State<_DiagnosticsDialog> {
  late bool _trace = traceNextRun(widget.folder);

  @override
  Widget build(BuildContext context) {
    final body = widget.report == null ? '${widget.error ?? 'No diagnostics'}' : pretty(widget.report!);
    return ContentDialog(
      title: Row(children: [
        const Icon(LucideIcons.bug, color: HsColors.brand),
        const SizedBox(width: 8),
        Text('Diagnostics for ${widget.folder}'),
      ]),
      content: SizedBox(
        width: 760,
        height: 520,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            MergeSemantics(
              child: Row(
                children: [
                  Checkbox(
                    checked: _trace,
                    onChanged: (value) {
                      setState(() => _trace = value ?? false);
                      setTraceNextRun(widget.folder, _trace);
                    },
                  ),
                  const SizedBox(width: 8),
                  const Expanded(
                    child: Text('Trace the next game run (HOS_TRACE=1 in hookshot_env.txt)',
                        style: TextStyle(fontSize: 12.5, color: HsColors.inkSoft)),
                  ),
                ],
              ),
            ),
            const SizedBox(height: 10),
            Expanded(
              child: Container(
                decoration: BoxDecoration(
                  color: HsColors.logBg,
                  borderRadius: BorderRadius.circular(6),
                  border: Border.all(color: HsColors.glassLine),
                ),
                padding: const EdgeInsets.all(10),
                child: SelectableText(body,
                    style: const TextStyle(fontSize: 11, color: HsColors.inkSoft, fontFamilyFallback: hsMonoFamilies)),
              ),
            ),
          ],
        ),
      ),
      actions: [
        Button(
          onPressed: () => openFolder(widget.folder),
          child: const Row(mainAxisSize: MainAxisSize.min, children: [
            Icon(LucideIcons.folderOpen, size: 14),
            SizedBox(width: 6),
            Text('Open folder'),
          ]),
        ),
        Button(
          onPressed: () async {
            await Clipboard.setData(ClipboardData(text: body));
            if (context.mounted) Navigator.pop(context);
          },
          child: const Row(mainAxisSize: MainAxisSize.min, children: [
            Icon(LucideIcons.copy, size: 14),
            SizedBox(width: 6),
            Text('Copy'),
          ]),
        ),
        FilledButton(onPressed: () => Navigator.pop(context), child: const Text('Close')),
      ],
    );
  }
}
