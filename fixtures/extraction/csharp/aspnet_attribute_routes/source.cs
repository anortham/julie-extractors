using Microsoft.AspNetCore.Mvc;

namespace Sample.Api;

[ApiController]
[Route("api/[controller]")]
public class UsersController : ControllerBase
{
    [HttpGet("{id}")]
    public IActionResult Get(int id) => Ok();

    [HttpPost]
    public IActionResult Create() => Ok();

    [HttpGet("[action]")]
    public IActionResult List() => Ok();

    [Route("legacy")]
    public IActionResult Legacy() => Ok();

    // Non-literal templates stay silent (constant reference, interpolation).
    [HttpGet(Routes.Ping)]
    public IActionResult Ping() => Ok();

    [HttpGet($"/computed/{Version}")]
    public IActionResult Computed() => Ok();
}

// A plain [ApiController] with no route attributes emits nothing.
[ApiController]
public class HealthController : ControllerBase
{
    public IActionResult Index() => Ok();

    [HttpGet("ping")]
    public IActionResult Ping() => Ok();
}
