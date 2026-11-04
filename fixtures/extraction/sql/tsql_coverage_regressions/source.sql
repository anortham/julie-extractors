CREATE PROCEDURE dbo.p @Id int WITH EXECUTE AS OWNER AS BEGIN SELECT @Id; END;

CREATE PROCEDURE dbo.TransactionWork AS
BEGIN
    BEGIN TRAN;
    SELECT Id FROM dbo.T;
    COMMIT;
    BEGIN TRAN;
    ROLLBACK;
END;

CREATE TABLE dbo.T (
    Id int NOT NULL,
    ValidFrom datetime2 GENERATED ALWAYS AS ROW START NOT NULL,
    ValidTo datetime2 GENERATED ALWAYS AS ROW END NOT NULL,
    PERIOD FOR SYSTEM_TIME (ValidFrom, ValidTo)
) WITH (SYSTEM_VERSIONING = ON);

CREATE TABLE [Application].[Cities] (
    [CityID] int CONSTRAINT [DF_Cities_CityID] DEFAULT (NEXT VALUE FOR [Sequences].[CityID]) NOT NULL,
    [StateProvinceID] int NOT NULL,
    CONSTRAINT [PK_Cities] PRIMARY KEY CLUSTERED ([CityID], [StateProvinceID]),
    CONSTRAINT [FK_Cities_StateProvince] FOREIGN KEY ([StateProvinceID]) REFERENCES [Dimension].[City] ([CityID])
);

CREATE FUNCTION dbo.GenerateDateDimensionColumns(@Year int)
RETURNS TABLE
AS
RETURN SELECT CASE
    WHEN EXISTS (SELECT 1 FROM dbo.T WHERE Id = @Year) THEN @Year
    ELSE 0
END AS Value;

CREATE FUNCTION [Application].DetermineCustomerAccess(@CityID int)
RETURNS TABLE
WITH SCHEMABINDING
AS
RETURN (
    SELECT 1 AS AccessResult
    FROM dbo.T
    WHERE EXISTS (
        SELECT 1
        FROM dbo.T AS Allowed
        WHERE Allowed.Id = dbo.T.Id AND dbo.T.Id = @CityID
    )
);

CREATE FUNCTION [DataLoadSimulation].[GetAreaCode](@PhoneNumber nvarchar(20))
RETURNS NVARCHAR(4)
WITH EXECUTE AS OWNER
AS BEGIN
    DECLARE @AreaCode AS NVARCHAR(4);
    SELECT @AreaCode = SUBSTRING(@PhoneNumber, 1, 4);
    RETURN @AreaCode;
END;
