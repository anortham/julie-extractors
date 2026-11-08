# Razor

Razor C# expressions use the C# variable-access rules. Reads, writes, and compound writes are classified on the guest syntax before their source spans are published in Razor coordinates. Component tags and Razor directives remain markup or type references without access metadata.

## Comments

A `@* … *@` comment is one parser token and an extra, so it is accepted in every position the Razor compiler accepts it: above the directives, first in an element, first in an `@if`, `else`, or `@foreach` block, and first in a `@code` block. The `comment_placement` fixture holds each position, and none of them produces a parse diagnostic or hides a `@code` member. A block that holds only a comment, or nothing, is still a parse error.
