CREATE TABLE dbo.Customers (
    CustomerId INT NOT NULL PRIMARY KEY,
    Email NVARCHAR(256) NOT NULL
);
GO
CREATE TABLE dbo.Orders (
    OrderId INT NOT NULL PRIMARY KEY,
    CustomerId INT NOT NULL,
    Total DECIMAL(18,2) NOT NULL
);
GO
ALTER TABLE [dbo].[Orders] ADD CONSTRAINT [FK_Orders_Customers] FOREIGN KEY ([CustomerId]) REFERENCES [dbo].[Customers] ([CustomerId]);
GO
ALTER TABLE dbo.Orders ADD PlacedAt DATETIME2 NULL;
GO
CREATE PROCEDURE dbo.usp_WriteAudit (@Entity NVARCHAR(50))
AS
BEGIN
    INSERT INTO audit.AuditLog (Entity) VALUES (@Entity);
END;
GO
CREATE PROCEDURE dbo.usp_PlaceOrder
    @CustomerId INT,
    @Total DECIMAL(18,2)
AS
BEGIN
    INSERT INTO dbo.Orders (CustomerId, Total) VALUES (@CustomerId, @Total);
    UPDATE dbo.Customers SET Email = Email WHERE CustomerId = @CustomerId;
    DELETE FROM dbo.OrderDrafts WHERE CustomerId = @CustomerId;
    EXEC dbo.usp_WriteAudit N'order';
    EXECUTE billing.usp_Charge @CustomerId, @Total;
END
GO
-- Answers a health probe.
CREATE PROC dbo.usp_Ping AS SELECT 1;
GO
ALTER PROCEDURE dbo.usp_NightlyTotals (@Day DATE)
AS
BEGIN
    DELETE FROM dbo.DailyTotals WHERE Day = @Day;
END;
GO
ALTER FUNCTION dbo.fn_Label (@x INT)
RETURNS NVARCHAR(20)
AS
BEGIN
    RETURN N'label'
END;
GO
CREATE TRIGGER dbo.trg_Orders_Audit ON dbo.Orders AFTER INSERT AS BEGIN SELECT 1; END
GO
