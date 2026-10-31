<?php
$global_plain = 0; $global_plain = 1;
class Holder { public $value; }
function consume($value) {}
function writes($initial, $box, $argument, $records) {
 $initialized = $initial;
 $plain = 0; $plain = $initial;
 $compound = 0; $compound += $initial;
 $box->value = $initial;
 consume($argument);
 foreach ($records as &$captured) { consume($captured); }
}
