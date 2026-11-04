<?xml version="1.0"?>
<stylesheet xmlns="http://www.w3.org/1999/XSL/Transform"
            xmlns:q="http://www.w3.org/1999/XSL/Transform"
            xmlns:foreign="urn:foreign"
            version="2.0">
  <template name="decide">
    <param name="input"/>
    <if test="$input">
      <choose>
        <when test="$input &gt; 1">
          <for-each select="items/item">
            <value-of select="."/>
          </for-each>
        </when>
        <when test="$input = 1">
          <value-of select="$input"/>
        </when>
        <otherwise>none</otherwise>
      </choose>
    </if>
    <for-each-group select="items/item" group-by="@kind">
      <iterate select="current-group()">
        <value-of select="."/>
      </iterate>
    </for-each-group>
    <call-template name="unresolved">
      <with-param name="extra" select="$input"/>
    </call-template>
  </template>
  <q:template match="/">
    <q:for-each select="items/item">
      <q:if test="@active">
        <q:value-of select="."/>
      </q:if>
    </q:for-each>
  </q:template>
  <q:template name="shadowed" xmlns:q="urn:foreign">
    <q:if test="@active"/>
  </q:template>
  <q:function name="double">
    <q:param name="number"/>
    <q:sequence select="$number * 2"/>
  </q:function>
  <foreign:if><foreign:for-each/></foreign:if>
  <node note="if choose for-each"/>
  <!-- <if/><when/><for-each/> -->
  <literal><![CDATA[<if/><when/><for-each/>]]></literal>
</stylesheet>
