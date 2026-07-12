using Microsoft.AspNetCore.Components;

namespace Sample.Components;

public partial class Widget
{
    [Inject]
    public NavigationManager Navigation { get; set; } = default!;

    [CascadingParameter]
    public ThemeContext Theme { get; set; } = default!;

    public void Refresh()
    {
        Navigation.NavigateTo("/widgets/refresh");
    }
}
