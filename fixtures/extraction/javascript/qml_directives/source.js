// SPDX-License-Identifier: GPL-3.0-or-later
.pragma library
.import QtQuick 2.0 as QQ
.import "Geometry.js" as Geometry

function clamp(value, min, max) {
  var n = Number(value)
  if (!isFinite(n)) return min
  return Math.max(min, Math.min(max, n))
}

function screenWidth(screen) {
  return clamp(QQ.width(screen), 0, Geometry.maxWidth)
}
