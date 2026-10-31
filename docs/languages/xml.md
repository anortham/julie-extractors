# XML

XML has no native program-variable access sites. MSBuild `$(Name)` and Ant `${name}` property substitutions are explicit reads and carry `metadata_json.access = "read"`. Schema QNames, resource names, MyBatis mapper references, and other XML-domain links remain without access metadata. Embedded SQL in mapper content is extracted using SQL syntax.
