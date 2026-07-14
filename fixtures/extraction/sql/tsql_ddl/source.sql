IF SCHEMA_ID(N'app') IS NULL EXEC(N'CREATE SCHEMA [app];');
GO

CREATE TABLE [app].[Items] (
    [Id] int NOT NULL IDENTITY,
    [Status] nvarchar(50) NOT NULL,
    [Payload] varbinary(max) NULL,
    [Note] nvarchar(max) NULL,
    [LabelNormalized] AS UPPER([Status]) PERSISTED,
    CONSTRAINT PK_Items PRIMARY KEY ([Id])
);

CREATE TABLE dbo.Links (
    [ParentId] int NOT NULL,
    [ChildId] int NOT NULL,
    CONSTRAINT PK_Links PRIMARY KEY ([ParentId], [ChildId]),
    CONSTRAINT FK_Links_Parent FOREIGN KEY ([ParentId]) REFERENCES [app].[Items] ([Id])
);
