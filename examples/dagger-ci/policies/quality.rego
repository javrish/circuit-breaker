package quality

import future.keywords.in
import future.keywords.if
import future.keywords.contains

# ESLint outputs an array of file results
# Each file has: filePath, messages, errorCount, warningCount, etc.

default allow := false

# Allow if total error count across all files is zero
allow if {
    total_errors == 0
}

# Calculate total errors from all files
total_errors := sum([file.errorCount | some file in input.lint])

# Calculate total warnings from all files
total_warnings := sum([file.warningCount | some file in input.lint])

# Generate violation messages for files with errors
violation contains msg if {
    some file in input.lint
    file.errorCount > 0
    msg := sprintf("%s: %d error(s)", [file.filePath, file.errorCount])
}

# Generate warning messages (informational, doesn't block)
warning contains msg if {
    some file in input.lint
    file.warningCount > 0
    msg := sprintf("%s: %d warning(s)", [file.filePath, file.warningCount])
}

# Summary for reporting
summary := {
    "totalErrors": total_errors,
    "totalWarnings": total_warnings,
    "filesChecked": count(input.lint),
    "vcs": input.vcs,
}
