SET NOCOUNT ON;
GO

IF OBJECT_ID(N'dbo.Seed', N'U') IS NULL
BEGIN
    CREATE TABLE dbo.Seed (
        [Area] nvarchar(50) NOT NULL,
        [Role] nvarchar(50) NOT NULL,
        CONSTRAINT PK_Seed PRIMARY KEY ([Area], [Role])
    );
END
GO

MERGE dbo.Seed AS t
USING (VALUES (N'alpha', N'Reader'), (N'beta', N'Admin')) AS s (Area, Role)
ON t.Area = s.Area AND t.Role = s.Role
WHEN NOT MATCHED THEN INSERT (Area, Role) VALUES (s.Area, s.Role);
GO

SET XACT_ABORT ON;
DECLARE @Token nvarchar(64) = N'bootstrap';

IF EXISTS (SELECT 1 FROM dbo.Seed WHERE Area = N'missing')
    THROW 50001, N'invalid seed row', 1;

IF COL_LENGTH('dbo.Seed', 'Extra') IS NULL
BEGIN
    ALTER TABLE dbo.Seed ADD Extra int NULL;
END
GO
