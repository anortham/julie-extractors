CREATE TABLE dbo.Orders (
    [Id] int NOT NULL,
    [Status] nvarchar(50) NOT NULL DEFAULT N'new',
    [Total] decimal(18,2) NOT NULL
);
GO
CREATE INDEX [IX_Orders_Status] ON [dbo].[Orders] ([Status]) INCLUDE ([Total]) WHERE [Total] > 0;
GO
EXEC tSQLt.NewTestClass 'OrderTests';
GO
CREATE PROCEDURE OrderTests.SetUp
AS
BEGIN
    EXEC tSQLt.FakeTable 'dbo.Orders';
END;
GO
CREATE PROCEDURE OrderTests.[test drain empties the queue]
AS
BEGIN
    EXEC dbo.usp_Drain @Batch = 10;
END;
GO
CREATE PROCEDURE dbo.usp_Drain (@Batch INT)
AS
BEGIN
    DECLARE @Removed INT, @Label NVARCHAR(50);
    WHILE EXISTS (SELECT 1 FROM dbo.Orders o WHERE o.Total < @Batch)
    BEGIN
        DELETE FROM dbo.Orders WHERE Total < @Batch;
    END
    INSERT INTO dbo.Orders (Id, Status, Total) VALUES (@Batch, N'drained', 0);
END;
GO
MERGE INTO dbo.Orders AS t
USING dbo.Incoming AS s ON t.Id = s.Id
WHEN MATCHED THEN UPDATE SET Total = s.Total
WHEN NOT MATCHED THEN INSERT (Id, Status, Total) VALUES (s.Id, s.Status, s.Total);
GO
IF SCHEMA_ID(N'audit') IS NULL EXEC(N'CREATE SCHEMA [audit];');
GO
CREATE TRIGGER dbo.trg_Orders_Audit ON dbo.Orders
AFTER INSERT, UPDATE
AS
BEGIN
    EXEC dbo.usp_WriteAudit @Entity = N'Order';
END;
GO
