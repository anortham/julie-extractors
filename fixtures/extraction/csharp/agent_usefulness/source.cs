var group = app.MapGroup("/api");
group.MapMethods("/items", new[] { "GET", "POST" }, HandleItems);
app.MapMethods("/collection", [HttpMethods.Head, "OPTIONS"], HandleCollection);
app.MapHead("/head", HandleHead);
app.MapOptions("/options", HandleOptions);
app.MapMethods("/unknown", methods, HandleUnknown);
