Feature: Crouching, going prone, and what they do to the gun

  Background:
    Given I am in test room 1
    And I have the smg

  Scenario: C crouches, and C again stands up
    When I press C
    Then I am crouching
    And my eyes are below 1.4 metres
    When I press C
    Then I am standing
    And my eyes are above 1.7 metres

  Scenario: Z goes prone, and Z again stands up
    When I press Z
    Then I am prone
    And my eyes are below 0.7 metres
    When I press Z
    Then I am standing

  Scenario: Jumping from prone stands you up instead
    Given I press Z
    And I am prone
    When I press Space
    Then I am standing

  Scenario: Crouching is slower than walking
    Given I press C
    And I am crouching
    When I hold W
    Then I am moving slower than 4 metres per second

  Scenario: Going prone is slower still
    Given I press Z
    And I am prone
    When I hold W
    Then I am moving slower than 2 metres per second

  Scenario: Standing and walking is the normal pace
    When I hold W
    Then I am moving faster than 5 metres per second

  Scenario: Crouching steadies the gun
    Given the crosshair is more than 1.5 degrees wide
    When I press C
    Then the crosshair is less than 1.3 degrees wide

  Scenario: Going prone steadies it further
    When I press Z
    Then the crosshair is less than 0.9 degrees wide

  Scenario: Moving opens the crosshair
    Given the crosshair is less than 2 degrees wide
    When I hold W
    Then the crosshair is more than 2.2 degrees wide

  Scenario: Jumping opens the crosshair
    Given the crosshair is less than 2 degrees wide
    When I press Space
    Then the crosshair is more than 3 degrees wide

  Scenario: Going on the sights closes the crosshair right down
    Given the crosshair is more than 1.5 degrees wide
    When I right click
    Then the crosshair is less than 0.3 degrees wide
