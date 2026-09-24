class Result:
 pass
func make() -> Result:
 return Result.new()
func run():
 var made = make()
 var awaited := await self.make()
