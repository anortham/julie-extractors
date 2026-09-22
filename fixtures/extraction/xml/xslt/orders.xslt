<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:import href="common.xsl" />
  <xsl:variable name="currency" select="'USD'" />
  <xsl:template match="/">
    <xsl:call-template name="header">
      <xsl:with-param name="title" select="'Orders'" />
    </xsl:call-template>
    <xsl:call-template name="footer" />
    <xsl:element name="summary" />
  </xsl:template>
  <!-- Page header with a title. -->
  <xsl:template name="header">
    <xsl:param name="title" />
  </xsl:template>
</xsl:stylesheet>
