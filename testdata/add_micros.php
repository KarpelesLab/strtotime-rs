#!/usr/bin/env php
<?php
/**
 * Adds an `expected_micros` column to strtotime_tests.csv without changing any
 * existing row. The microsecond fraction is a property of the input's literal
 * time field (independent of base/timezone), so we derive it from the input:
 *   - "@<ts>.<frac>"  -> DateTime's microseconds (timelib truncates ns -> us)
 *   - everything else -> date_parse()'s `fraction` (also us-truncated; false for
 *     relative/now inputs, which never carry a fraction)
 *
 * expected_unix stays exactly as PHP's strtotime() produced it.
 */

$dir = __DIR__;
$path = "$dir/strtotime_tests.csv";

/** Microseconds (0..999999) carried by an input's literal time field. */
function micros_of(string $input): int {
    $input = trim($input);
    if ($input !== '' && $input[0] === '@') {
        try {
            $dt = new DateTime($input);
            return (int)$dt->format('u');
        } catch (Throwable $e) {
            return 0; // >6 frac digits etc. — handled as invalid elsewhere
        }
    }
    $p = @date_parse($input);
    if (is_array($p) && empty($p['errors']) && !empty($p['fraction'])) {
        return (int)round($p['fraction'] * 1_000_000);
    }
    return 0;
}

$fh = fopen($path, 'r');
if (!$fh) {
    fwrite(STDERR, "cannot open $path\n");
    exit(1);
}
$rows = [];
$header = fgetcsv($fh);
while (($row = fgetcsv($fh)) !== false) {
    if (count($row) < 4) continue;
    $rows[] = $row;
}
fclose($fh);

$out = fopen($path, 'w');
fputcsv($out, ['input', 'base_unix', 'tz', 'expected_unix', 'expected_micros']);
$with_frac = 0;
foreach ($rows as $row) {
    [$input, $base, $tz, $unix] = $row;
    $micros = micros_of($input);
    if ($micros !== 0) $with_frac++;
    fputcsv($out, [$input, $base, $tz, $unix, (string)$micros]);
}
fclose($out);

fprintf(STDERR, "Wrote %d rows (%d with non-zero micros)\n", count($rows), $with_frac);
