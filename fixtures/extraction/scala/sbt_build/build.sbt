ThisBuild / scalaVersion := "3.3.1"

lazy val core = (project in file("core"))
  .settings(
    name := "acme-core",
    libraryDependencies ++= Seq("org.typelevel" %% "cats-core" % "2.10.0")
  )

lazy val root = (project in file(".")).aggregate(core).dependsOn(core)
