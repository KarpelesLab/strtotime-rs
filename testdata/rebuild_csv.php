#!/usr/bin/env php
<?php
/**
 * Rebuilds strtotime_tests.csv and strtotime_invalid.csv using PHP's strtotime()
 * as the source of truth. Entries that PHP rejects go to invalid, entries PHP
 * accepts go to success with PHP's actual return value.
 */

$dir = __DIR__;
$success = [];
$invalid = [];
$removed = [];

// --- Process success CSV ---
$fh = fopen("$dir/strtotime_tests.csv", 'r');
fgetcsv($fh); // skip header
while (($row = fgetcsv($fh)) !== false) {
    if (count($row) < 4) continue;
    [$input, $baseUnix, $tz, $expectedUnix] = $row;
    processEntry($input, (int)$baseUnix, $tz, $success, $invalid, $removed);
}
fclose($fh);

// --- Process invalid CSV ---
$fh = fopen("$dir/strtotime_invalid.csv", 'r');
fgetcsv($fh); // skip header
while (($row = fgetcsv($fh)) !== false) {
    if (count($row) < 3) continue;
    [$input, $baseUnix, $tz] = $row;
    processEntry($input, (int)$baseUnix, $tz, $success, $invalid, $removed);
}
fclose($fh);

// --- Write success CSV ---
$fh = fopen("$dir/strtotime_tests.csv", 'w');
fputcsv($fh, ['input', 'base_unix', 'tz', 'expected_unix']);
// Sort by expected_unix for easy dedup
usort($success, function($a, $b) {
    if ($a[3] !== $b[3]) return $a[3] <=> $b[3];
    if ($a[0] !== $b[0]) return $a[0] <=> $b[0];
    if ($a[1] !== $b[1]) return $a[1] <=> $b[1];
    return $a[2] <=> $b[2];
});
// Dedup
$prev = null;
$written = 0;
foreach ($success as $row) {
    $key = implode('|', $row);
    if ($key === $prev) continue;
    $prev = $key;
    fputcsv($fh, $row);
    $written++;
}
fclose($fh);

// --- Write invalid CSV ---
$fh = fopen("$dir/strtotime_invalid.csv", 'w');
fputcsv($fh, ['input', 'base_unix', 'tz']);
// Dedup
$seen = [];
foreach ($invalid as $row) {
    $key = implode('|', $row);
    if (isset($seen[$key])) continue;
    $seen[$key] = true;
    fputcsv($fh, $row);
}
fclose($fh);

fprintf(STDERR, "Written: %d success, %d invalid, %d removed\n", $written, count($invalid) - count($seen) + count($seen), count($removed));
foreach ($removed as $r) {
    fprintf(STDERR, "  removed: %s\n", $r);
}

function processEntry(string $input, int $baseUnix, string $tz, array &$success, array &$invalid, array &$removed): void {
    // Fix timezone: +01:00 → CET for PHP compatibility
    if ($tz === '+01:00') $tz = 'CET';

    // Set timezone
    if ($tz !== '') {
        if (!@date_default_timezone_set($tz)) {
            $removed[] = "$input (unknown tz: $tz)";
            return;
        }
    } else {
        date_default_timezone_set('UTC');
    }

    // Call strtotime
    if ($baseUnix !== 0) {
        $result = @strtotime($input, $baseUnix);
    } else {
        $result = @strtotime($input);
    }

    if ($result === false) {
        // PHP rejects this input
        $invalid[] = [$input, (string)$baseUnix, $tz];
    } else {
        // PHP accepts — use PHP's value as expected
        $success[] = [$input, (string)$baseUnix, $tz, (string)$result];
    }
}
