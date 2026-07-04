import { Controller, Get, Post, Delete, All } from '@nestjs/common';

@Controller('users')
export class UsersController {
  @Get()
  findAll() {
    return [];
  }

  @Get(':id')
  findOne() {
    return null;
  }

  @Post()
  create() {
    return null;
  }

  @Delete(':id')
  remove() {
    return null;
  }

  @All('audit')
  audit() {
    return null;
  }

  // Negative: interpolated decorator arguments stay silent.
  @Get(`/tpl/${id}`)
  interpolated() {
    return null;
  }

  // Negative: concatenated decorator arguments stay silent.
  @Post('/a/' + suffix)
  concatenated() {
    return null;
  }
}
