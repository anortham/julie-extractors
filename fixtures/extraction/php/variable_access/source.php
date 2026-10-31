<?php
$global_plain = 0; $global_plain = 1;
class Holder { public $value; public $count; }
function consume($value) {}
function writes($initial, $box, $argument, $records) {
 $initialized = $initial;
 $plain = 0; $plain = $initial;
 $compound = 0; $compound += $initial;
 $box->value = $initial;
 $box->count++;
 $box->count--;
 $items = [0];
 $index = 0;
 $items[$index]++;
 $items[$index]--;
 $counter = 0;
 $counter++;
 $counter--;
 consume($argument);
 foreach ($records as &$captured) { consume($captured); }
}
