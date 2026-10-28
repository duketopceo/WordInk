@{
    # The installer is an interactive console script: it reports with Write-Host, and its helper
    # nouns and switch parameters are deliberate (tests dot-source it).
    ExcludeRules = @('PSAvoidUsingWriteHost', 'PSReviewUnusedParameter', 'PSUseSingularNouns', 'PSUseShouldProcessForStateChangingFunctions')
}
