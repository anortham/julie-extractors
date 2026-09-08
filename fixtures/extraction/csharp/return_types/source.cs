class Factory {
 public Result Bare() => new Result();
 public Result? Nullable() => null;
 public Result[] Array() => null;
 public Box<Result> Generic() => null;
 public Models.Result Qualified() => null;
}
