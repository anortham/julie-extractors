INSERT INTO dbo.Workers (Id, Name) VALUES (2, N'fixture-insert');
DELETE FROM dbo.Jobs WHERE WorkerId = 2;

CREATE PROCEDURE dbo.MarkWorker
    @Id INT
AS
BEGIN
    UPDATE dbo.Workers SET Name = N'marked' WHERE Id = @Id;
END;

CREATE FUNCTION dbo.WorkerLabel(@Id INT)
RETURNS NVARCHAR(100)
AS
BEGIN
    RETURN N'worker';
END;

SELECT Id, ROW_NUMBER() OVER (PARTITION BY Name ORDER BY Id) AS Rn
FROM dbo.Workers;

SELECT Id
FROM dbo.Workers
WINDOW recent AS (PARTITION BY Name ORDER BY Id);
