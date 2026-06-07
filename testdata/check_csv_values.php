#!/usr/bin/env php
<?php
/**
 * Validates strtotime_tests.csv and strtotime_invalid.csv against PHP's strtotime().
 *
 * Usage: php testdata/check_csv_values.php
 *
 * This library's purpose is to match PHP's strtotime() exactly.
 * Every test case must produce the same result as PHP. No skips.
 */

$dir = __DIR__;
$pass = 0;
$fail = 0;

// --- Validate success cases ---
$fh = fopen("$dir/strtotime_tests.csv", 'r');
if (!$fh) {
    fwrite(STDERR, "Cannot open strtotime_tests.csv\n");
    exit(2);
}

fgetcsv($fh); // skip header
$line = 1;

while (($row = fgetcsv($fh)) !== false) {
    $line++;
    if (count($row) < 4) continue;

    [$input, $baseUnix, $tz, $expectedUnix] = $row;
    $baseUnix = (int)$baseUnix;
    $expectedUnix = (int)$expectedUnix;

    // Set timezone context
    if ($tz !== '') {
        if (!@date_default_timezone_set($tz)) {
            fprintf(STDERR, "FAIL line %d: unknown timezone %s for %s\n", $line, json_encode($tz), json_encode($input));
            $fail++;
            continue;
        }
    } else {
        date_default_timezone_set('UTC');
    }

    if ($baseUnix !== 0) {
        $result = @strtotime($input, $baseUnix);
    } else {
        $result = @strtotime($input);
    }

    if ($result === false) {
        fprintf(STDERR, "FAIL line %d: PHP rejects %s (expected %d)\n", $line, json_encode($input), $expectedUnix);
        $fail++;
        continue;
    }

    $diff = abs($result - $expectedUnix);
    if ($diff > 1) {
        fprintf(STDERR, "FAIL line %d: %s => PHP=%d, expected=%d (diff=%ds)\n",
            $line, json_encode($input), $result, $expectedUnix, $result - $expectedUnix);
        $fail++;
    } else {
        $pass++;
    }
}
fclose($fh);

// --- Validate invalid cases ---
$fh = fopen("$dir/strtotime_invalid.csv", 'r');
if ($fh) {
    fgetcsv($fh); // skip header
    $line = 1;
    while (($row = fgetcsv($fh)) !== false) {
        $line++;
        if (count($row) < 3) continue;

        [$input, $baseUnix, $tz] = $row;
        $baseUnix = (int)$baseUnix;

        if ($tz !== '') {
            if (!@date_default_timezone_set($tz)) {
                fprintf(STDERR, "FAIL invalid line %d: unknown timezone %s\n", $line, json_encode($tz));
                $fail++;
                continue;
            }
        } else {
            date_default_timezone_set('UTC');
        }

        if ($baseUnix !== 0) {
            $result = @strtotime($input, $baseUnix);
        } else {
            $result = @strtotime($input);
        }

        if ($result !== false) {
            fprintf(STDERR, "FAIL invalid line %d: PHP accepts %s = %d, but Go rejects it\n", $line, json_encode($input), $result);
            $fail++;
        } else {
            $pass++;
        }
    }
    fclose($fh);
}

// --- Summary ---
fprintf(STDERR, "\nResults: %d passed, %d failed\n", $pass, $fail);
exit($fail > 0 ? 1 : 0);
