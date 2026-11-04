// region comment
/// region doc
let café = 1
let plain = "/* plain string content */"
let multiline = """
// multiline string content
/* multiline block content */
"""
let interpolated = "value \(/* interpolation comment */ café)"
let raw = #"raw /* raw text */ \#(/* raw interpolation comment */ café)"#
