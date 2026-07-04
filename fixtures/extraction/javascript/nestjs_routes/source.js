import { Controller, Get, Post } from '@nestjs/common';

@Controller('health')
class HealthController {
  @Get()
  liveness() {
    return 'ok';
  }

  @Get(':check')
  readiness() {
    return 'ok';
  }

  @Post('reset')
  reset() {
    return 'ok';
  }

  // Negative: an identifier reference is not a static route.
  @Get(routePath)
  dynamic() {
    return null;
  }
}
